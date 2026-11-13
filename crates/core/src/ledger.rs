//! SQLite persistence

use crate::jobs::{Job, JobStatus};
use crate::tools::geotag::sync::{Tombstone, TrackStamp};
use crate::tools::geotag::{DayCoverage, PointSource, SourcedPoint, TrackPoint};
use rusqlite::{Connection, OptionalExtension, Result as SqlResult};
use std::collections::HashMap;
use std::path::Path;

/// One disagreement, and what was decided about it.
///
/// Written whenever a file offers a different position for an instant the
/// library already holds. These are not supposed to happen — every fix comes
/// from one phone — so when one does, this row is the only record of why the
/// library says something one of its own stored files does not.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackConflictRecord {
    pub at: i64,
    /// The position that is in the timeline now.
    pub kept: TrackPoint,
    /// The position that lost.
    pub other: TrackPoint,
    pub metres: f64,
    /// `kept-existing` or `took-new`.
    pub decision: String,
}

fn track_row(row: &rusqlite::Row) -> SqlResult<TrackRow> {
    let corners: (Option<f64>, Option<f64>, Option<f64>, Option<f64>) =
        (row.get(11)?, row.get(12)?, row.get(13)?, row.get(14)?);
    Ok(TrackRow {
        id: row.get(0)?,
        name: row.get(1)?,
        source_path: row.get(2)?,
        creator: row.get(3)?,
        imported_at: row.get(4)?,
        point_count: row.get(5)?,
        points_added: row.get(6)?,
        points_identical: row.get(7)?,
        points_conflicting: row.get(8)?,
        first_fix: row.get(9)?,
        last_fix: row.get(10)?,
        bounds: match corners {
            (Some(a), Some(b), Some(c), Some(d)) => Some((a, b, c, d)),
            _ => None,
        },
        source: PointSource::from_label(&row.get::<_, String>(15)?),
    })
}

/// Forward-only schema migrations.
///
/// Each entry is applied in order exactly once; `PRAGMA user_version` records
/// how many have run. **Never edit or reorder an entry that has shipped** —
/// append a new one instead, or databases in the field will diverge from fresh
/// ones. Index 0 is schema version 1.
const MIGRATIONS: &[&str] = &[
    // 1 — initial schema, specification §7.
    r#"
    CREATE TABLE IF NOT EXISTS users (
        uid TEXT PRIMARY KEY,
        display_name TEXT,
        added_at INTEGER
    );

    CREATE TABLE IF NOT EXISTS cards (
        id TEXT PRIMARY KEY,
        volume_label TEXT,
        fingerprint TEXT,
        first_seen INTEGER,
        last_seen INTEGER
    );

    CREATE TABLE IF NOT EXISTS shots (
        id TEXT PRIMARY KEY,
        card_id TEXT,
        stem TEXT,
        candidate_asset_id TEXT,
        status TEXT
    );

    CREATE TABLE IF NOT EXISTS assets (
        id TEXT PRIMARY KEY,
        shot_id TEXT,
        rel_path TEXT,
        kind TEXT,
        bytes INTEGER,
        sha256 TEXT,
        capture_datetime INTEGER,
        width INTEGER,
        height INTEGER,
        camera TEXT
    );

    CREATE TABLE IF NOT EXISTS checks (
        shot_id TEXT,
        name TEXT,
        status TEXT,
        detail TEXT,
        PRIMARY KEY (shot_id, name)
    );

    CREATE TABLE IF NOT EXISTS derived (
        shot_id TEXT PRIMARY KEY,
        staged_path TEXT,
        sha256 TEXT,
        bytes INTEGER,
        width INTEGER,
        height INTEGER
    );

    CREATE TABLE IF NOT EXISTS publishes (
        shot_id TEXT PRIMARY KEY,
        upload_token TEXT,
        media_item_id TEXT,
        state TEXT,
        attempts INTEGER,
        error TEXT
    );

    CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        kind TEXT,
        state TEXT,
        progress INTEGER,
        total INTEGER,
        started_at INTEGER,
        finished_at INTEGER,
        error TEXT
    );

    CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );

    CREATE TABLE IF NOT EXISTS oauth (
        provider TEXT PRIMARY KEY,
        encrypted_refresh_token TEXT,
        scope TEXT,
        expires_at INTEGER
    );
    "#,
    // 2 — indexes for the lookups the ingest and publish paths actually make.
    r#"
    CREATE INDEX IF NOT EXISTS idx_assets_sha256    ON assets (sha256);
    CREATE INDEX IF NOT EXISTS idx_assets_shot_id   ON assets (shot_id);
    CREATE INDEX IF NOT EXISTS idx_shots_card_id    ON shots (card_id);
    CREATE INDEX IF NOT EXISTS idx_jobs_state       ON jobs (state);
    "#,
    // 3 — the physical card a shot came from, as distinct from the observation
    // of it. F10 identifies a card by label *plus contents*, so `card_id`
    // changes the moment another frame is shot; a shot needs a key that does
    // not, or a reinserted card reports every frame on it as new.
    r#"
    ALTER TABLE shots ADD COLUMN card_scope TEXT;
    CREATE INDEX IF NOT EXISTS idx_shots_card_scope ON shots (card_scope);
    "#,
    // 4 — F16's deduplication ledger, and the handoff sessions that write to it.
    //
    // Specification §7 lists neither table. It does list `publishes`, but that
    // is keyed by `shot_id` and holds the Google Photos state machine; F16 asks
    // for something different and says so plainly — *"the server maintains a
    // SHA-256 ledger of every file it has published"* — and a key that is a shot
    // on one particular card cannot answer "have I published this photograph
    // before?" for a card that has been reformatted since. Recorded as a gap in
    // the phase report rather than resolved by editing the specification (G9).
    //
    // The key is the **source** hash: the bytes the camera wrote, which never
    // change; a derived file's hash changes with every re-encode. The handoff
    // that wrote these rows was retired in RS-1 (2026-11-14); the tables stay,
    // so a NAS database keeps its history.
    r#"
    CREATE TABLE IF NOT EXISTS published (
        source_sha256 TEXT PRIMARY KEY,
        stem TEXT,
        derived_sha256 TEXT,
        session_id TEXT,
        media_item_id TEXT,
        published_at INTEGER
    );

    CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        card_id TEXT,
        state TEXT,
        created_at INTEGER,
        manifest TEXT,
        plan TEXT,
        report TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_sessions_state ON sessions (state);
    "#,
    // 5 — F15's publishing. Three additions, each for one requirement.
    //
    // `oauth.state` is §6.2's reconnect path: catching `invalid_grant` has to
    // leave a mark somewhere, or the next of four hundred photographs asks
    // Google the same dead question again.
    //
    // `sessions.dry_run_at` is §9.2 rule 3, which makes a dry run mandatory
    // before publishing. It has to be *persisted*: the API cannot delete, so a
    // dry run remembered only in a process that has since restarted is no
    // safeguard at all.
    //
    // `publishes.session_id` lets a publish job find its own shots. §7 lists the
    // table without it, because §7 predates sessions existing.
    r#"
    ALTER TABLE oauth ADD COLUMN state TEXT;
    ALTER TABLE sessions ADD COLUMN dry_run_at INTEGER;
    ALTER TABLE publishes ADD COLUMN session_id TEXT;
    ALTER TABLE publishes ADD COLUMN source_sha256 TEXT;
    ALTER TABLE publishes ADD COLUMN file_name TEXT;
    ALTER TABLE publishes ADD COLUMN stem TEXT;

    CREATE INDEX IF NOT EXISTS idx_publishes_session ON publishes (session_id);
    CREATE INDEX IF NOT EXISTS idx_publishes_state   ON publishes (state);
    "#,
    // 6 — a job's closing summary.
    //
    // The string a job returns ("dry run: 4 files would be redated") went out
    // on the live event and nowhere else, so anything that subscribed after
    // the job finished could only ever be told "done". A preview whose result
    // is unreadable a second later is not a preview.
    r#"
    ALTER TABLE jobs ADD COLUMN summary TEXT;
    "#,
    // 7 — the GPS track library (`docs/geotag-plan.md`).
    //
    // **Beyond the specification**, which mentions neither GPS nor GPX; recorded
    // in `docs/known-gaps.md` rather than resolved by editing it (G9).
    //
    // `track_points.at` is the primary key, and that is the whole design: one
    // position per instant, enforced here rather than remembered by the
    // importer. Every fix comes from one phone, so a second file offering a
    // different position for a second already held is a fault to put to the
    // user — and expressing that as a key collision is what makes it impossible
    // to resolve silently by whichever import ran last.
    //
    // The GPX text is kept beside the parsed points. It costs a few kilobytes
    // and buys provenance — the file can be handed back byte for byte to
    // whoever asks where a coordinate came from — and a second chance at
    // anything the reader ignores today.
    r#"
    CREATE TABLE IF NOT EXISTS tracks (
        id                 TEXT PRIMARY KEY,
        name               TEXT,
        source_path        TEXT,
        creator            TEXT,
        imported_at        INTEGER,
        point_count        INTEGER,
        points_added       INTEGER,
        points_identical   INTEGER,
        points_conflicting INTEGER,
        first_fix          INTEGER,
        last_fix           INTEGER,
        min_lat REAL, min_lon REAL, max_lat REAL, max_lon REAL,
        gpx                TEXT
    );

    CREATE TABLE IF NOT EXISTS track_points (
        at       INTEGER PRIMARY KEY,
        lat      REAL NOT NULL,
        lon      REAL NOT NULL,
        ele      REAL,
        track_id TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS point_conflicts (
        at         INTEGER NOT NULL,
        track_id   TEXT NOT NULL,
        kept_lat REAL, kept_lon REAL, kept_ele REAL,
        other_lat REAL, other_lon REAL, other_ele REAL,
        metres     REAL,
        decision   TEXT,
        decided_at INTEGER,
        PRIMARY KEY (at, track_id)
    );

    CREATE INDEX IF NOT EXISTS idx_track_points_track_id ON track_points (track_id);
    "#,
    // 8 — which kind of key a `published` row is keyed on.
    //
    // F16's ledger answers "have I published this before?" against the **source**
    // hash: the bytes the camera wrote, which nothing rewrites. Publishing a
    // folder cannot use that key — the files in it have been through the tools,
    // and geotagging in particular rewrites a file, so one photograph has a
    // different hash after it is tagged than before.
    //
    // A folder publish therefore keys on the bytes it uploaded, which answers a
    // weaker question: "have I uploaded exactly this file?" rather than "have I
    // published this photograph?". Both rows live in one table because both
    // answer "do not upload this again", and the column is what stops a later
    // reader mistaking one guarantee for the other.
    //
    // Existing rows predate folder publishing and are all source-keyed.
    r#"
    ALTER TABLE published ADD COLUMN key_kind TEXT NOT NULL DEFAULT 'source';
    "#,
    // 9 — where a folder publish's session row records its folder.
    //
    // A folder publish needs a session row because §9.2 rule 3's dry-run record
    // hangs off one, but it has no card. Storing the path in `card_id` would be
    // a lie in a column name a later reader would believe.
    //
    // **This belongs to migration 8 and is a separate entry because 8 had
    // already run.** Adding it to 8 after the fact left every database that had
    // seen 8 without the column, at `user_version = 8`, and therefore never
    // catching up — which is precisely what the warning at the top of this list
    // is about. The cost of getting it wrong is a table that exists with a
    // column missing, and an error that names neither cause nor remedy.
    r#"
    ALTER TABLE sessions ADD COLUMN folder TEXT;
    "#,
    // 10 — where a fix came from (`docs/timeline-plan.md`).
    //
    // `join.rs` promises that every position it writes was *recorded*. Points
    // placed by hand on the Timeline tab are not recordings — they are the
    // photographer asserting where they were — and Google's place visits are an
    // inference from signals nobody can see. All three are worth having and
    // they are not the same claim, so the claim is stored rather than left to
    // be guessed from a track's name.
    //
    // On `tracks` rather than on `track_points`: a point is attributed to the
    // import that contributed it, so the column belongs to the import and a
    // join answers for the point. One row per import instead of one per fix,
    // and a timeline of a million points does not carry the word "recorded" a
    // million times.
    //
    // Every existing row is a `.gpx` off a phone, which is exactly `recorded`.
    r#"
    ALTER TABLE tracks ADD COLUMN source TEXT NOT NULL DEFAULT 'recorded';
    "#,
    // 11 — deletions, remembered (`docs/timeline-sync-plan.md`).
    //
    // The Mac and the NAS each hold a timeline and neither is the authority, so
    // a sync that only added would resurrect: delete a bad track here, sync,
    // and the other side hands it straight back. A deletion is a statement
    // about a track and has to survive as one.
    //
    // Kept forever, and cheap to: a row is a hash and an integer, and somebody
    // who has deleted a thousand tracks has a 60 KB table. Dropping old ones
    // would reintroduce exactly the resurrection this exists to stop.
    r#"
    CREATE TABLE IF NOT EXISTS deleted_tracks (
        id         TEXT PRIMARY KEY,
        deleted_at INTEGER NOT NULL
    );
    "#,
    // 12 — fixes a phone reported, before they are a track
    // (`docs/owntracks-plan.md`).
    //
    // Not written straight into `track_points`, for two reasons. Every position
    // in the timeline came from a document whose text is stored beside it —
    // that is what lets somebody ask where a coordinate came from — and a fix
    // that arrives over HTTP has no document until the day it belongs to is
    // complete. And `at` as the key makes a retry free: a phone that posts,
    // loses the reply and posts again writes the same row twice with no
    // consequence.
    //
    // Emptied a day at a time, as each day becomes a track.
    r#"
    CREATE TABLE IF NOT EXISTS device_fixes (
        at          INTEGER PRIMARY KEY,
        lat         REAL NOT NULL,
        lon         REAL NOT NULL,
        ele         REAL,
        device      TEXT,
        received_at INTEGER NOT NULL
    );
    "#,
];

/// One imported GPX file.
///
/// The counts are what the import actually did, not what the file held: a
/// second export of the same afternoon can be fifty points that add nothing,
/// and a row saying so is the answer to "did that import work?"
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRow {
    /// The sha256 of the file's bytes, which is what makes importing idempotent.
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub creator: Option<String>,
    pub imported_at: i64,
    pub point_count: i64,
    pub points_added: i64,
    pub points_identical: i64,
    pub points_conflicting: i64,
    pub first_fix: Option<i64>,
    pub last_fix: Option<i64>,
    pub bounds: Option<(f64, f64, f64, f64)>,
    /// Recorded, placed by hand, or inferred — migration 10.
    pub source: PointSource,
}

pub struct Ledger {
    conn: Connection,
}

impl Ledger {
    pub fn open<P: AsRef<Path>>(path: P) -> SqlResult<Self> {
        let conn = Connection::open(path)?;

        // A second connection to the same file is a normal thing here — a
        // publish job writes its own rows while the job runner writes progress
        // through another — and without a busy timeout the loser of that race
        // gets `SQLITE_BUSY` immediately rather than waiting the moment out.
        conn.busy_timeout(std::time::Duration::from_secs(10))?;

        Self::apply_migrations(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> SqlResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::apply_migrations(&conn)?;
        Ok(Self { conn })
    }

    /// Apply any migration the database has not seen yet.
    fn apply_migrations(conn: &Connection) -> SqlResult<()> {
        let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        for (index, migration) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            conn.execute_batch(migration)?;
            // pragma_update will not accept a bound parameter here.
            conn.execute_batch(&format!("PRAGMA user_version = {}", index + 1))?;
        }
        Ok(())
    }

    /// The schema version this database is at — the number of migrations applied.
    pub fn schema_version(&self) -> SqlResult<i64> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
    }

    pub fn inner(&self) -> &Connection {
        &self.conn
    }

    // ---------------------------------------------------------------- users

    pub fn add_user(&self, uid: &str, display_name: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO users (uid, display_name, added_at)
             VALUES (?1, ?2, strftime('%s', 'now'))",
            (uid, display_name),
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- cards

    /// Record a card sighting. First sighting sets `first_seen`; every later
    /// sighting refreshes `last_seen`.
    pub fn upsert_card(
        &self,
        id: &str,
        volume_label: Option<&str>,
        fingerprint: &str,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO cards (id, volume_label, fingerprint, first_seen, last_seen)
             VALUES (?1, ?2, ?3, strftime('%s', 'now'), strftime('%s', 'now'))
             ON CONFLICT(id) DO UPDATE SET
                 last_seen = strftime('%s', 'now'),
                 volume_label = COALESCE(excluded.volume_label, cards.volume_label)",
            (id, volume_label, fingerprint),
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- shots

    /// Record a shot against both the physical card and the observation of it.
    ///
    /// `scope` identifies the card across shooting sessions; `card_id` is the
    /// state it was in when this scan ran, and is refreshed on every re-scan so
    /// the row points at the most recent observation.
    pub fn upsert_shot(&self, id: &str, scope: &str, card_id: &str, stem: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO shots (id, card_id, card_scope, stem, status)
             VALUES (?1, ?3, ?2, ?4, 'new')
             ON CONFLICT(id) DO UPDATE SET card_id = excluded.card_id",
            (id, scope, card_id, stem),
        )?;
        Ok(())
    }

    /// Record which asset of a shot is the one that will be published (F11).
    pub fn set_shot_candidate(&self, shot_id: &str, asset_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE shots SET candidate_asset_id = ?2 WHERE id = ?1",
            (shot_id, asset_id),
        )?;
        Ok(())
    }

    // --------------------------------------------------------------- assets

    /// Write an asset row with every column the data model defines.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_asset(
        &self,
        id: &str,
        shot_id: &str,
        rel_path: &str,
        kind: &str,
        bytes: u64,
        sha256: &str,
        capture_datetime: Option<i64>,
        width: Option<u32>,
        height: Option<u32>,
        camera: Option<&str>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO assets
                 (id, shot_id, rel_path, kind, bytes, sha256,
                  capture_datetime, width, height, camera)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                shot_id,
                rel_path,
                kind,
                bytes,
                sha256,
                capture_datetime,
                width,
                height,
                camera
            ],
        )?;
        Ok(())
    }

    /// How many rows a table holds.
    ///
    /// SQLite cannot bind an identifier, so the table name is interpolated. It
    /// is asserted to be a bare identifier for that reason: this is only ever
    /// called with a literal from this crate, and the assertion is what keeps it
    /// that way.
    pub fn count(&self, table: &str) -> SqlResult<i64> {
        assert!(
            table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "table names are literals, not input: {table:?}"
        );
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
    }

    /// The filename stems already recorded for a physical card.
    ///
    /// Used by detection to say how many shots on a reinserted card are new
    /// (F10), without reading a single photograph. Keyed by scope rather than
    /// `card_id` — see [`upsert_shot`](Self::upsert_shot).
    pub fn shot_stems(&self, scope: &str) -> SqlResult<std::collections::HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT stem FROM shots WHERE card_scope = ?1")?;
        let rows = stmt.query_map([scope], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    /// Shots with no candidate recorded — a scan that did not complete, or a
    /// shot whose only asset could not be read (F11).
    pub fn shots_without_candidate(&self) -> SqlResult<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM shots WHERE candidate_asset_id IS NULL",
            [],
            |row| row.get(0),
        )
    }

    // ----------------------------------------------------------------- jobs

    /// Persist a new job. F17 — a job exists in the ledger before any work starts,
    /// so a crash mid-operation still leaves a recoverable record.
    pub fn insert_job(&self, job: &Job) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO jobs
                 (id, kind, state, progress, total, started_at, finished_at, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                job.id,
                job.kind,
                job.status.as_str(),
                job.progress,
                job.total,
                job.started_at,
                job.finished_at,
                job.error,
            ],
        )?;
        Ok(())
    }

    pub fn update_job_progress(&self, id: &str, progress: u64, total: u64) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE jobs SET state = 'running', progress = ?2, total = ?3 WHERE id = ?1",
            (id, progress, total),
        )?;
        Ok(())
    }

    /// Move a job to a terminal state and stamp `finished_at`.
    pub fn finish_job(
        &self,
        id: &str,
        status: JobStatus,
        error: Option<&str>,
        summary: Option<&str>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE jobs
                SET state = ?2, error = ?3, summary = ?4,
                    finished_at = strftime('%s', 'now')
              WHERE id = ?1",
            (id, status.as_str(), error, summary),
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> SqlResult<Option<Job>> {
        self.conn
            .query_row(
                "SELECT id, kind, state, progress, total, started_at, finished_at, error, summary
                   FROM jobs WHERE id = ?1",
                [id],
                Self::row_to_job,
            )
            .optional()
    }

    pub fn jobs_with_status(&self, status: JobStatus) -> SqlResult<Vec<Job>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, state, progress, total, started_at, finished_at, error, summary
               FROM jobs WHERE state = ?1 ORDER BY started_at",
        )?;
        let rows = stmt.query_map([status.as_str()], Self::row_to_job)?;
        rows.collect()
    }

    /// Called once at startup. Any job still `pending` or `running` belonged to a
    /// process that is no longer alive: mark it `interrupted` and return it.
    ///
    /// F17 — an interrupted job resumes or reports failure, and never silently
    /// disappears. This is the "reports failure" half; a resumable job kind can
    /// pick its record back up from the returned list.
    pub fn recover_interrupted_jobs(&self) -> SqlResult<Vec<Job>> {
        self.conn.execute(
            "UPDATE jobs
                SET state = 'interrupted',
                    error = COALESCE(error, 'process stopped before the job finished'),
                    finished_at = strftime('%s', 'now')
              WHERE state IN ('pending', 'running')",
            [],
        )?;
        self.jobs_with_status(JobStatus::Interrupted)
    }

    fn row_to_job(row: &rusqlite::Row<'_>) -> SqlResult<Job> {
        let state: String = row.get(2)?;
        let status = state.parse::<JobStatus>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(Job {
            id: row.get(0)?,
            kind: row.get(1)?,
            status,
            progress: row.get(3)?,
            total: row.get(4)?,
            started_at: row.get(5)?,
            finished_at: row.get(6)?,
            error: row.get(7)?,
            summary: row.get(8)?,
        })
    }

    // -----------------------------------------------------------------------
    // The GPS track library (`docs/geotag-plan.md`)
    // -----------------------------------------------------------------------

    /// Every imported track, most recent first.
    pub fn tracks(&self) -> SqlResult<Vec<TrackRow>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, source_path, creator, imported_at, point_count,
                    points_added, points_identical, points_conflicting,
                    first_fix, last_fix, min_lat, min_lon, max_lat, max_lon, source
               FROM tracks ORDER BY imported_at DESC, id",
        )?;
        let rows = statement.query_map([], track_row)?;
        rows.collect()
    }

    /// One track, by the hash of the file it came from.
    pub fn track(&self, id: &str) -> SqlResult<Option<TrackRow>> {
        self.conn
            .query_row(
                "SELECT id, name, source_path, creator, imported_at, point_count,
                        points_added, points_identical, points_conflicting,
                        first_fix, last_fix, min_lat, min_lon, max_lat, max_lon, source
                   FROM tracks WHERE id = ?1",
                [id],
                track_row,
            )
            .optional()
    }

    /// The fixes recorded in a window, in time order.
    ///
    /// The window is what keeps a library of a year of five-minute fixes from
    /// being loaded to place four hundred photographs from one afternoon.
    pub fn points_between(&self, from: i64, to: i64) -> SqlResult<Vec<TrackPoint>> {
        let mut statement = self.conn.prepare(
            "SELECT at, lat, lon, ele FROM track_points
              WHERE at BETWEEN ?1 AND ?2 ORDER BY at",
        )?;
        let rows = statement.query_map([from, to], |row| {
            Ok(TrackPoint {
                at: row.get(0)?,
                lat: row.get(1)?,
                lon: row.get(2)?,
                ele: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// The fixes in a window, each with the import that contributed it.
    ///
    /// What `points_between` answers, plus what a map has to say about a point
    /// before somebody trusts it: which file it came from, and whether it was
    /// recorded, placed by hand or inferred (migration 10).
    pub fn points_with_source(&self, from: i64, to: i64) -> SqlResult<Vec<SourcedPoint>> {
        let mut statement = self.conn.prepare(
            "SELECT p.at, p.lat, p.lon, p.ele, p.track_id, t.name, t.source
               FROM track_points p
               JOIN tracks t ON t.id = p.track_id
              WHERE p.at BETWEEN ?1 AND ?2
              ORDER BY p.at",
        )?;
        let rows = statement.query_map([from, to], |row| {
            Ok(SourcedPoint {
                point: TrackPoint {
                    at: row.get(0)?,
                    lat: row.get(1)?,
                    lon: row.get(2)?,
                    ele: row.get(3)?,
                },
                track_id: row.get(4)?,
                track_name: row.get(5)?,
                source: PointSource::from_label(&row.get::<_, String>(6)?),
            })
        })?;
        rows.collect()
    }

    /// How many fixes each day of a window holds.
    ///
    /// `offset_seconds` is east of UTC, and it is applied to the *grouping*
    /// rather than to each row on the way out: a day is a day where the person
    /// was, and a month of European evenings grouped by UTC days would report
    /// the small hours of the next one. Counted in SQL, so a year answers with
    /// 365 rows rather than a million.
    ///
    /// **Days with no fixes are absent, not zero** — the caller knows which days
    /// it asked about, and a blank day is exactly what this is for.
    pub fn coverage(&self, from: i64, to: i64, offset_seconds: i64) -> SqlResult<Vec<DayCoverage>> {
        let mut statement = self.conn.prepare(
            // Integer division truncates towards zero, so a negative offset
            // before 1970 would bucket backwards; the floor is taken with the
            // same arithmetic SQLite has, rather than a CAST that rounds.
            "SELECT ((at + ?3) - ((at + ?3) % 86400)) / 86400 AS day, COUNT(*)
               FROM track_points
              WHERE at BETWEEN ?1 AND ?2
              GROUP BY day ORDER BY day",
        )?;
        let rows = statement.query_map([from, to, offset_seconds], |row| {
            let day: i64 = row.get(0)?;
            Ok(DayCoverage {
                day: day * 86400 - offset_seconds,
                fixes: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// The first and last instant the library holds anything for.
    ///
    /// What a screen opens on: a library with fixes should not open on today
    /// when today is blank and 2013 is where everything is.
    pub fn timeline_extent(&self) -> SqlResult<Option<(i64, i64)>> {
        let extent =
            self.conn
                .query_row("SELECT MIN(at), MAX(at) FROM track_points", [], |row| {
                    Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
                })?;
        Ok(match extent {
            (Some(first), Some(last)) => Some((first, last)),
            _ => None,
        })
    }

    /// The last fix at or before an instant, and the first at or after it.
    ///
    /// The pair a windowed read cannot supply on its own. A window is chosen
    /// from the photographs, and the fixes bracketing them can be any distance
    /// outside it — an overnight leaves ten hours — so without these two a
    /// refusal would report "after the last fix in the library" for a
    /// photograph the library has fixes on both sides of. A tool that has to
    /// refuse should at least refuse for the true reason.
    pub fn points_around(
        &self,
        from: i64,
        to: i64,
    ) -> SqlResult<(Option<TrackPoint>, Option<TrackPoint>)> {
        let read = |row: &rusqlite::Row| {
            Ok(TrackPoint {
                at: row.get(0)?,
                lat: row.get(1)?,
                lon: row.get(2)?,
                ele: row.get(3)?,
            })
        };
        let before = self
            .conn
            .query_row(
                "SELECT at, lat, lon, ele FROM track_points
                  WHERE at < ?1 ORDER BY at DESC LIMIT 1",
                [from],
                read,
            )
            .optional()?;
        let after = self
            .conn
            .query_row(
                "SELECT at, lat, lon, ele FROM track_points
                  WHERE at > ?1 ORDER BY at ASC LIMIT 1",
                [to],
                read,
            )
            .optional()?;
        Ok((before, after))
    }

    /// The fix held at each of these instants, and which track it came from.
    ///
    /// The read behind the import diff. Asked instant by instant rather than as
    /// a range because an export can cover a fortnight with one afternoon
    /// missing, and a range would drag the fortnight through memory to compare
    /// fifty points.
    pub fn points_at(&self, instants: &[i64]) -> SqlResult<HashMap<i64, (TrackPoint, String)>> {
        let mut held = HashMap::new();
        let mut statement = self
            .conn
            .prepare("SELECT at, lat, lon, ele, track_id FROM track_points WHERE at = ?1")?;
        for at in instants {
            let found = statement
                .query_row([at], |row| {
                    Ok((
                        TrackPoint {
                            at: row.get(0)?,
                            lat: row.get(1)?,
                            lon: row.get(2)?,
                            ele: row.get(3)?,
                        },
                        row.get::<_, String>(4)?,
                    ))
                })
                .optional()?;
            if let Some(found) = found {
                held.insert(*at, found);
            }
        }
        Ok(held)
    }

    /// Record an import: the file, the fixes it contributes, and every
    /// disagreement it turned up.
    ///
    /// **One transaction.** A half-applied import leaves a timeline nobody
    /// chose — some of the file's points in, the decisions about the rest
    /// unrecorded, and no way to tell from the outside which half happened.
    ///
    /// `points` are written with `INSERT OR REPLACE`, so the caller decides
    /// what is in that list: an instant the user chose to keep as it stands is
    /// simply not passed.
    pub fn record_track_import(
        &self,
        row: &TrackRow,
        gpx: &str,
        points: &[TrackPoint],
        conflicts: &[TrackConflictRecord],
    ) -> SqlResult<()> {
        let transaction = self.conn.unchecked_transaction()?;

        transaction.execute(
            "INSERT OR REPLACE INTO tracks
                 (id, name, source_path, creator, imported_at, point_count,
                  points_added, points_identical, points_conflicting,
                  first_fix, last_fix, min_lat, min_lon, max_lat, max_lon, gpx, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            rusqlite::params![
                row.id,
                row.name,
                row.source_path,
                row.creator,
                row.imported_at,
                row.point_count,
                row.points_added,
                row.points_identical,
                row.points_conflicting,
                row.first_fix,
                row.last_fix,
                row.bounds.map(|b| b.0),
                row.bounds.map(|b| b.1),
                row.bounds.map(|b| b.2),
                row.bounds.map(|b| b.3),
                gpx,
                row.source.as_str(),
            ],
        )?;

        for point in points {
            transaction.execute(
                "INSERT OR REPLACE INTO track_points (at, lat, lon, ele, track_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![point.at, point.lat, point.lon, point.ele, row.id],
            )?;
        }

        // A track being imported is the most recent statement about it, so any
        // tombstone this side remembers is now out of date. Without this a
        // re-imported file would be deleted again by the next sync, using a
        // deletion its owner had already overruled.
        transaction.execute("DELETE FROM deleted_tracks WHERE id = ?1", [&row.id])?;

        for conflict in conflicts {
            transaction.execute(
                "INSERT OR REPLACE INTO point_conflicts
                     (at, track_id, kept_lat, kept_lon, kept_ele,
                      other_lat, other_lon, other_ele, metres, decision, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    conflict.at,
                    row.id,
                    conflict.kept.lat,
                    conflict.kept.lon,
                    conflict.kept.ele,
                    conflict.other.lat,
                    conflict.other.lon,
                    conflict.other.ele,
                    conflict.metres,
                    conflict.decision,
                    row.imported_at,
                ],
            )?;
        }

        transaction.commit()
    }

    /// Forget a track and the fixes still attributed to it.
    ///
    /// Points a later file also contained are attributed to whichever import
    /// first contributed them, so this can remove a position another stored
    /// file also attests to. The GPX text of both is kept, which makes that
    /// recoverable by re-importing — a deliberate limit, chosen over a table of
    /// attestations, because every fix here comes from one phone.
    /// `now` is written as the tombstone's date, so a re-import afterwards can
    /// be recognised as the later statement (`geotag::sync`).
    pub fn delete_track(&self, id: &str, now: i64) -> SqlResult<usize> {
        let transaction = self.conn.unchecked_transaction()?;
        let removed = transaction.execute("DELETE FROM track_points WHERE track_id = ?1", [id])?;
        transaction.execute("DELETE FROM point_conflicts WHERE track_id = ?1", [id])?;
        transaction.execute("DELETE FROM tracks WHERE id = ?1", [id])?;
        // In the same transaction as the deletion: a tombstone written
        // separately could be lost by a crash between the two, leaving a
        // deletion the other machine would undo on the next sync.
        transaction.execute(
            "INSERT OR REPLACE INTO deleted_tracks (id, deleted_at) VALUES (?1, ?2)",
            rusqlite::params![id, now],
        )?;
        transaction.commit()?;
        Ok(removed)
    }

    /// Store a fix a device reported (`docs/owntracks-plan.md`).
    ///
    /// `INSERT OR IGNORE`: one position per instant, and a phone that posts the
    /// same fix twice — because it never saw the first reply — says nothing
    /// new. Returns whether this one was new, which is what the endpoint
    /// reports and what a test asserts.
    pub fn record_device_fix(
        &self,
        point: &TrackPoint,
        device: Option<&str>,
        received_at: i64,
    ) -> SqlResult<bool> {
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO device_fixes (at, lat, lon, ele, device, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                point.at,
                point.lat,
                point.lon,
                point.ele,
                device,
                received_at
            ],
        )?;
        Ok(changed == 1)
    }

    /// The staged fixes in a window, in time order.
    pub fn device_fixes_between(&self, from: i64, to: i64) -> SqlResult<Vec<TrackPoint>> {
        let mut statement = self.conn.prepare(
            "SELECT at, lat, lon, ele FROM device_fixes
              WHERE at BETWEEN ?1 AND ?2 ORDER BY at",
        )?;
        let rows = statement.query_map([from, to], |row| {
            Ok(TrackPoint {
                at: row.get(0)?,
                lat: row.get(1)?,
                lon: row.get(2)?,
                ele: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// Which device reported in a window, for naming the day's track.
    ///
    /// The commonest name rather than all of them: two phones on one account is
    /// not what this is for, and a track called "iPhone, iPad" would be a
    /// guess dressed as a fact.
    pub fn device_in(&self, from: i64, to: i64) -> SqlResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT device FROM device_fixes
                  WHERE at BETWEEN ?1 AND ?2 AND device IS NOT NULL
                  GROUP BY device ORDER BY COUNT(*) DESC LIMIT 1",
                [from, to],
                |row| row.get(0),
            )
            .optional()
    }

    /// The instants of the oldest and newest staged fix, if any are waiting.
    pub fn device_fix_extent(&self) -> SqlResult<Option<(i64, i64)>> {
        let extent =
            self.conn
                .query_row("SELECT MIN(at), MAX(at) FROM device_fixes", [], |row| {
                    Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
                })?;
        Ok(match extent {
            (Some(first), Some(last)) => Some((first, last)),
            _ => None,
        })
    }

    /// Forget the staged fixes of a window, once they are a stored track.
    pub fn clear_device_fixes(&self, from: i64, to: i64) -> SqlResult<usize> {
        self.conn.execute(
            "DELETE FROM device_fixes WHERE at BETWEEN ?1 AND ?2",
            [from, to],
        )
    }

    /// Every deletion this side remembers, for a sync inventory.
    pub fn tombstones(&self) -> SqlResult<Vec<Tombstone>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, deleted_at FROM deleted_tracks ORDER BY deleted_at")?;
        let rows = statement.query_map([], |row| {
            Ok(Tombstone {
                id: row.get(0)?,
                deleted_at: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// What this side holds, in the form a sync compares.
    ///
    /// No points and no GPX text — an inventory decides *which* tracks move,
    /// and the fixes are fetched only for those.
    pub fn track_stamps(&self) -> SqlResult<Vec<TrackStamp>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, imported_at, point_count,
                    gpx IS NOT NULL AND length(gpx) > 0
               FROM tracks ORDER BY imported_at, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(TrackStamp {
                id: row.get(0)?,
                name: row.get(1)?,
                imported_at: row.get(2)?,
                point_count: row.get(3)?,
                has_text: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// The stored GPX text of a track, for handing it to the other machine.
    pub fn track_text(&self, id: &str) -> SqlResult<Option<String>> {
        self.conn
            .query_row("SELECT gpx FROM tracks WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
    }

    /// Every disagreement recorded against a track, oldest instant first.
    pub fn conflicts_for_track(&self, id: &str) -> SqlResult<Vec<TrackConflictRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT at, kept_lat, kept_lon, kept_ele, other_lat, other_lon, other_ele,
                    metres, decision
               FROM point_conflicts WHERE track_id = ?1 ORDER BY at",
        )?;
        let rows = statement.query_map([id], |row| {
            let at: i64 = row.get(0)?;
            Ok(TrackConflictRecord {
                at,
                kept: TrackPoint {
                    at,
                    lat: row.get(1)?,
                    lon: row.get(2)?,
                    ele: row.get(3)?,
                },
                other: TrackPoint {
                    at,
                    lat: row.get(4)?,
                    lon: row.get(5)?,
                    ele: row.get(6)?,
                },
                metres: row.get(7)?,
                decision: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            (key, value),
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> SqlResult<Option<String>> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_once_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");

        let ledger = Ledger::open(&path).unwrap();
        assert_eq!(ledger.schema_version().unwrap(), MIGRATIONS.len() as i64);
        ledger.set_setting("survives", "yes").unwrap();
        drop(ledger);

        // Re-opening applies nothing further and destroys nothing.
        let reopened = Ledger::open(&path).unwrap();
        assert_eq!(reopened.schema_version().unwrap(), MIGRATIONS.len() as i64);
        assert_eq!(
            reopened.get_setting("survives").unwrap().as_deref(),
            Some("yes")
        );
    }

    #[test]
    fn a_database_at_an_older_version_is_migrated_forward() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.sqlite3");

        // Simulate a database created before migration 2 existed.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.execute_batch("PRAGMA user_version = 1").unwrap();
        }

        let ledger = Ledger::open(&path).unwrap();
        assert_eq!(ledger.schema_version().unwrap(), MIGRATIONS.len() as i64);

        let index_count: i64 = ledger
            .inner()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                  WHERE type = 'index' AND name = 'idx_assets_sha256'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 1, "migration 2 should have added the index");
    }

    /// Phase 1 acceptance: a job written, the process simulated as restarted,
    /// the job recovered.
    ///
    /// The restart is simulated by dropping the `Ledger` — closing the SQLite
    /// connection entirely — and opening a fresh one from the same file. Nothing
    /// is carried over in memory.
    #[test]
    fn a_job_survives_a_restart_and_is_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite3");

        let started_at;
        {
            let ledger = Ledger::open(&path).unwrap();
            let job = Job::new("job-running", "card_scan", 400);
            started_at = job.started_at;
            ledger.insert_job(&job).unwrap();
            ledger.update_job_progress("job-running", 137, 400).unwrap();

            // A job that finished cleanly before the crash.
            let done = Job::new("job-done", "date_scan", 10);
            ledger.insert_job(&done).unwrap();
            ledger
                .finish_job("job-done", JobStatus::Completed, None, None)
                .unwrap();
        } // process stops here

        let ledger = Ledger::open(&path).unwrap();

        // The record survived with its progress intact.
        let recovered = ledger.get_job("job-running").unwrap().unwrap();
        assert_eq!(recovered.status, JobStatus::Running);
        assert_eq!(recovered.progress, 137);
        assert_eq!(recovered.total, 400);
        assert_eq!(recovered.started_at, started_at);

        // Startup recovery reports it as interrupted rather than losing it.
        let interrupted = ledger.recover_interrupted_jobs().unwrap();
        assert_eq!(interrupted.len(), 1, "only the in-flight job is recovered");
        assert_eq!(interrupted[0].id, "job-running");
        assert_eq!(interrupted[0].status, JobStatus::Interrupted);
        assert!(interrupted[0].error.is_some(), "F17: never fails silently");
        assert!(interrupted[0].finished_at.is_some());
        assert_eq!(
            interrupted[0].progress, 137,
            "progress is preserved so the job can resume"
        );

        // The completed job was left alone.
        let done = ledger.get_job("job-done").unwrap().unwrap();
        assert_eq!(done.status, JobStatus::Completed);

        // Recovery is idempotent — a second startup finds nothing new in flight.
        assert_eq!(
            ledger.jobs_with_status(JobStatus::Running).unwrap().len(),
            0
        );
        assert_eq!(
            ledger.jobs_with_status(JobStatus::Pending).unwrap().len(),
            0
        );
    }

    #[test]
    fn an_unknown_job_is_none_not_an_error() {
        let ledger = Ledger::open_in_memory().unwrap();
        assert!(ledger.get_job("never-existed").unwrap().is_none());
    }

    /// A job's closing summary has to outlive the event that announced it.
    ///
    /// The dry run of a date repair reports what *would* change, and a person
    /// who opened the screen a second later used to be told only "done".
    #[test]
    fn a_finished_jobs_summary_is_readable_after_the_event_has_gone() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(dir.path().join("l.sqlite3")).unwrap();

        let job = Job::new("job-dry", "dates_fix", 4);
        ledger.insert_job(&job).unwrap();
        ledger
            .finish_job(
                "job-dry",
                JobStatus::Completed,
                None,
                Some("dry run: 4 files would be redated, 0 skipped"),
            )
            .unwrap();

        let read = ledger.get_job("job-dry").unwrap().unwrap();
        assert_eq!(read.status, JobStatus::Completed);
        assert_eq!(
            read.summary.as_deref(),
            Some("dry run: 4 files would be redated, 0 skipped"),
            "the result of the preview is what the screen has to show"
        );
    }

    /// A failure keeps both: the error to explain it, the summary to say how
    /// far it got.
    #[test]
    fn a_failed_job_keeps_its_error_and_its_summary_apart() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(dir.path().join("l.sqlite3")).unwrap();

        ledger
            .insert_job(&Job::new("job-bad", "rename_apply", 9))
            .unwrap();
        ledger
            .finish_job(
                "job-bad",
                JobStatus::Failed,
                Some("disk full"),
                Some("disk full"),
            )
            .unwrap();

        let read = ledger.get_job("job-bad").unwrap().unwrap();
        assert_eq!(read.error.as_deref(), Some("disk full"));
        assert_eq!(read.summary.as_deref(), Some("disk full"));
    }

    /// A track and its fixes, for the timeline queries below.
    fn store_track(ledger: &Ledger, id: &str, source: PointSource, fixes: &[(i64, f64, f64)]) {
        let row = TrackRow {
            id: id.into(),
            name: format!("{id}.gpx"),
            source_path: format!("/tracks/{id}.gpx"),
            creator: Some("OwnTracks".into()),
            imported_at: 1_700_000_000,
            point_count: fixes.len() as i64,
            points_added: fixes.len() as i64,
            points_identical: 0,
            points_conflicting: 0,
            first_fix: fixes.first().map(|f| f.0),
            last_fix: fixes.last().map(|f| f.0),
            bounds: None,
            source,
        };
        let points: Vec<TrackPoint> = fixes
            .iter()
            .map(|(at, lat, lon)| TrackPoint {
                at: *at,
                lat: *lat,
                lon: *lon,
                ele: None,
            })
            .collect();
        ledger
            .record_track_import(&row, "<gpx/>", &points, &[])
            .unwrap();
    }

    #[test]
    fn a_fix_carries_the_kind_of_claim_its_import_made() {
        let ledger = Ledger::open_in_memory().unwrap();
        store_track(
            &ledger,
            "phone",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4)],
        );
        store_track(
            &ledger,
            "byhand",
            PointSource::Placed,
            &[(2_000, 48.85, 2.35)],
        );

        let points = ledger.points_with_source(0, 9_999).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].source, PointSource::Recorded);
        assert_eq!(points[0].track_name, "phone.gpx");
        assert_eq!(points[1].source, PointSource::Placed);
        assert_eq!(points[1].track_id, "byhand");
    }

    #[test]
    fn a_track_imported_before_provenance_existed_reads_as_recorded() {
        // Migration 10 defaults the column, and what those rows are is exactly
        // what the default says: fixes off a phone, in a .gpx.
        let ledger = Ledger::open_in_memory().unwrap();
        store_track(
            &ledger,
            "old",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4)],
        );
        ledger
            .inner()
            .execute_batch("UPDATE tracks SET source = 'recorded' WHERE id = 'old'")
            .unwrap();

        assert_eq!(ledger.tracks().unwrap()[0].source, PointSource::Recorded);
    }

    #[test]
    fn a_word_this_build_has_never_heard_of_reads_as_recorded() {
        let ledger = Ledger::open_in_memory().unwrap();
        store_track(
            &ledger,
            "future",
            PointSource::Placed,
            &[(1_000, 52.5, 13.4)],
        );
        ledger
            .inner()
            .execute_batch("UPDATE tracks SET source = 'triangulated' WHERE id = 'future'")
            .unwrap();

        // The library opens and the point is described in the most conservative
        // terms available, rather than the read failing.
        assert_eq!(ledger.tracks().unwrap()[0].source, PointSource::Recorded);
    }

    #[test]
    fn coverage_counts_the_fixes_of_each_day_and_omits_the_empty_ones() {
        let ledger = Ledger::open_in_memory().unwrap();
        // 2013-05-01 10:00Z, 11:00Z, and one on 2013-05-03.
        store_track(
            &ledger,
            "berlin",
            PointSource::Recorded,
            &[
                (1_367_402_400, 52.5, 13.4),
                (1_367_406_000, 52.5, 13.4),
                (1_367_575_200, 52.5, 13.4),
            ],
        );

        let days = ledger.coverage(1_367_366_400, 1_367_625_600, 0).unwrap();
        assert_eq!(
            days.len(),
            2,
            "the blank day between them is absent, not zero"
        );
        assert_eq!(days[0].fixes, 2);
        assert_eq!(days[1].fixes, 1);
        assert_eq!(days[0].day, 1_367_366_400, "the start of 2013-05-01 UTC");
    }

    #[test]
    fn coverage_groups_by_the_day_the_person_was_living_not_by_utc() {
        let ledger = Ledger::open_in_memory().unwrap();
        // 2013-05-01 23:30 in Berlin (UTC+2) is 21:30Z — the same day there,
        // and the day after in UTC if the offset is ignored.
        store_track(
            &ledger,
            "evening",
            PointSource::Recorded,
            &[(1_367_443_800, 52.5, 13.4)],
        );

        let utc = ledger.coverage(0, i64::MAX, 0).unwrap();
        let berlin = ledger.coverage(0, i64::MAX, 2 * 3600).unwrap();
        assert_eq!(utc[0].day, 1_367_366_400, "2013-05-01 00:00Z");
        assert_eq!(berlin[0].day, 1_367_359_200, "2013-05-01 00:00+02:00");
    }

    #[test]
    fn the_extent_is_what_the_library_actually_holds() {
        let ledger = Ledger::open_in_memory().unwrap();
        assert_eq!(ledger.timeline_extent().unwrap(), None);

        store_track(
            &ledger,
            "t",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4), (9_000, 52.6, 13.5)],
        );
        assert_eq!(ledger.timeline_extent().unwrap(), Some((1_000, 9_000)));
    }

    #[test]
    fn deleting_a_track_is_remembered_so_a_sync_cannot_resurrect_it() {
        let ledger = Ledger::open_in_memory().unwrap();
        store_track(
            &ledger,
            "doomed",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4)],
        );

        ledger.delete_track("doomed", 5_000).unwrap();

        assert_eq!(ledger.tracks().unwrap().len(), 0);
        assert_eq!(
            ledger.tombstones().unwrap(),
            vec![Tombstone {
                id: "doomed".into(),
                deleted_at: 5_000
            }]
        );
    }

    #[test]
    fn importing_a_track_again_forgets_that_it_was_deleted() {
        // The re-import is the later statement about the track, and a
        // tombstone left behind would have the next sync delete it again.
        let ledger = Ledger::open_in_memory().unwrap();
        store_track(
            &ledger,
            "revived",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4)],
        );
        ledger.delete_track("revived", 5_000).unwrap();

        store_track(
            &ledger,
            "revived",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4)],
        );

        assert_eq!(ledger.tracks().unwrap().len(), 1);
        assert_eq!(ledger.tombstones().unwrap(), vec![]);
    }

    #[test]
    fn an_inventory_says_what_is_held_without_carrying_any_of_it() {
        let ledger = Ledger::open_in_memory().unwrap();
        store_track(
            &ledger,
            "one",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4)],
        );

        let stamps = ledger.track_stamps().unwrap();
        assert_eq!(stamps.len(), 1);
        assert_eq!(stamps[0].id, "one");
        assert_eq!(stamps[0].point_count, 1);
        assert!(stamps[0].has_text, "store_track wrote <gpx/>");
        assert_eq!(ledger.track_text("one").unwrap().as_deref(), Some("<gpx/>"));
    }

    #[test]
    fn a_track_whose_text_was_not_kept_says_so_rather_than_looking_sendable() {
        let ledger = Ledger::open_in_memory().unwrap();
        store_track(
            &ledger,
            "huge",
            PointSource::Recorded,
            &[(1_000, 52.5, 13.4)],
        );
        ledger
            .inner()
            .execute_batch("UPDATE tracks SET gpx = '' WHERE id = 'huge'")
            .unwrap();

        assert!(!ledger.track_stamps().unwrap()[0].has_text);
    }

    /// A database at **any** earlier version catches up to exactly the schema a
    /// fresh one is created with.
    ///
    /// Every version, not a chosen one. The check used to start at migration 6,
    /// and the bug it missed was a line appended to migration 8 *after* 8 had
    /// already run: databases that had seen the old 8 stayed at
    /// `user_version = 8` for ever and never gained the column, while a fresh
    /// database got it on creation. Nothing failed until something read the
    /// column, and the error named neither the cause nor the remedy.
    ///
    /// Comparing the whole of `sqlite_master` rather than looking for one
    /// table: a migration that adds a table and forgets its index leaves two
    /// databases that behave differently under load and identically under a
    /// test that only checks the table exists.
    #[test]
    fn a_database_at_any_version_catches_up_to_a_fresh_one() {
        let dir = tempfile::tempdir().unwrap();

        let schema_of = |ledger: &Ledger| -> Vec<(String, String)> {
            let mut statement = ledger
                .inner()
                .prepare(
                    "SELECT name, COALESCE(sql, '') FROM sqlite_master
                      WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
                )
                .unwrap();
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap();
            rows.map(|r| r.unwrap()).collect()
        };

        let fresh = Ledger::open(dir.path().join("fresh.sqlite3")).unwrap();
        let expected = schema_of(&fresh);

        for version in 1..MIGRATIONS.len() {
            let path = dir.path().join(format!("at-{version}.sqlite3"));
            {
                let conn = Connection::open(&path).unwrap();
                for migration in &MIGRATIONS[..version] {
                    conn.execute_batch(migration).unwrap();
                }
                conn.execute_batch(&format!("PRAGMA user_version = {version}"))
                    .unwrap();
            }

            let migrated = Ledger::open(&path).unwrap();
            assert_eq!(
                migrated.schema_version().unwrap(),
                MIGRATIONS.len() as i64,
                "a database at version {version} should catch up"
            );
            assert_eq!(
                schema_of(&migrated),
                expected,
                "a database at version {version} ended up with a different schema \
                 from a fresh one"
            );

            // And the tables work, rather than merely existing.
            assert_eq!(migrated.tracks().unwrap(), vec![]);
            assert_eq!(migrated.points_between(0, i64::MAX).unwrap(), vec![]);
            // A session row, as the retired folder publishing wrote it (RS-1): the table stays, so a
            // NAS database's old rows are kept, and it must still accept one.
            migrated
                .inner()
                .execute(
                    "INSERT OR IGNORE INTO sessions (id, folder, state, created_at)
                     VALUES (?1, ?2, 'folder', ?3)",
                    (format!("folder-{version}"), "/Publishing", 0),
                )
                .unwrap();
        }
    }

    /// Phase 1 acceptance: a round-trip test per table. All ten tables of
    /// specification §7, every column written and read back.
    #[test]
    fn every_table_round_trips() {
        // The checks, derived, publishes and oauth tables lost their API with RS-1; they stay in the
        // schema (no migration drops a table), and the migration test above still covers them.
        let ledger = Ledger::open_in_memory().unwrap();
        let conn = ledger.inner();

        // users
        ledger.add_user("uid-1", "Pablo").unwrap();
        let (name, added): (String, i64) = conn
            .query_row(
                "SELECT display_name, added_at FROM users WHERE uid = 'uid-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "Pablo");
        assert!(added > 0);

        // cards
        ledger
            .upsert_card("card-1", Some("EOS_DIGITAL"), "fp-abc")
            .unwrap();
        let (label, fp, first, last): (String, String, i64, i64) = conn
            .query_row(
                "SELECT volume_label, fingerprint, first_seen, last_seen
                   FROM cards WHERE id = 'card-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(label, "EOS_DIGITAL");
        assert_eq!(fp, "fp-abc");
        assert!(first > 0 && last > 0);

        // shots
        // `add_shot` went with the handoff (RS-1); the row it wrote, written directly.
        conn.execute(
            "INSERT OR IGNORE INTO shots (id, card_id, stem, status) VALUES (?1, ?2, ?3, 'new')",
            ("shot-1", "card-1", "IMG_1234"),
        )
        .unwrap();
        ledger.set_shot_candidate("shot-1", "asset-1").unwrap();
        let (card_id, stem, candidate, status): (String, String, String, String) = conn
            .query_row(
                "SELECT card_id, stem, candidate_asset_id, status FROM shots WHERE id = 'shot-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((card_id.as_str(), stem.as_str()), ("card-1", "IMG_1234"));
        assert_eq!(candidate, "asset-1");
        assert_eq!(status, "new");

        // assets — every column
        ledger
            .upsert_asset(
                "asset-1",
                "shot-1",
                "DCIM/100/IMG_1234.JPG",
                "jpeg",
                12_582_912,
                "sha-jpeg",
                Some(1_714_564_800),
                Some(6000),
                Some(4000),
                Some("PENTAX17"),
            )
            .unwrap();
        #[allow(clippy::type_complexity)]
        let (rel, kind, bytes, sha, capture, w, h, cam): (
            String,
            String,
            i64,
            String,
            i64,
            u32,
            u32,
            String,
        ) = conn
            .query_row(
                "SELECT rel_path, kind, bytes, sha256, capture_datetime, width, height, camera
                   FROM assets WHERE id = 'asset-1'",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(rel, "DCIM/100/IMG_1234.JPG");
        assert_eq!(kind, "jpeg");
        assert_eq!(bytes, 12_582_912);
        assert_eq!(sha, "sha-jpeg");
        assert_eq!(capture, 1_714_564_800);
        assert_eq!((w, h), (6000, 4000));
        assert_eq!(cam, "PENTAX17");

        // jobs — through the typed API
        let job = Job::new("job-1", "date_scan", 500);
        ledger.insert_job(&job).unwrap();
        ledger.update_job_progress("job-1", 250, 500).unwrap();
        ledger
            .finish_job("job-1", JobStatus::Completed, None, Some("1 file renamed"))
            .unwrap();
        let stored = ledger.get_job("job-1").unwrap().unwrap();
        assert_eq!(stored.kind, "date_scan");
        assert_eq!(stored.status, JobStatus::Completed);
        assert_eq!((stored.progress, stored.total), (250, 500));
        assert_eq!(stored.started_at, job.started_at);
        assert!(stored.finished_at.is_some());
        assert!(stored.error.is_none());

        // settings
        ledger.set_setting("auto_resize", "true").unwrap();
        assert_eq!(
            ledger.get_setting("auto_resize").unwrap().as_deref(),
            Some("true")
        );
        assert_eq!(ledger.get_setting("absent").unwrap(), None);
    }
}
