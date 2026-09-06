//! SQLite persistence

use crate::jobs::{Job, JobStatus};
use crate::tools::geotag::TrackPoint;
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
    // change. See `ingest::handoff::manifest::ManifestEntry` for why the derived
    // hash cannot do this job.
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
];

/// One provider's stored authorisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthGrant {
    pub encrypted_refresh_token: String,
    pub scope: String,
    /// When the grant was last stored, as Unix seconds.
    pub expires_at: i64,
    /// `connected` or `disconnected` (§6.2's reconnect path).
    pub state: String,
}

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
}

/// One row of the publish state machine (§6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRow {
    pub shot_id: String,
    pub stem: String,
    pub source_sha256: String,
    pub file_name: String,
    pub upload_token: Option<String>,
    pub media_item_id: Option<String>,
    pub state: String,
    pub attempts: i64,
    pub error: Option<String>,
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

    pub fn add_card(&self, id: &str, fingerprint: &str) -> SqlResult<()> {
        self.upsert_card(id, None, fingerprint)
    }

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

    pub fn add_shot(&self, id: &str, card_id: &str, stem: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO shots (id, card_id, stem, status) VALUES (?1, ?2, ?3, 'new')",
            (id, card_id, stem),
        )?;
        Ok(())
    }

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

    pub fn add_asset(
        &self,
        id: &str,
        shot_id: &str,
        rel_path: &str,
        bytes: u64,
        sha256: &str,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO assets (id, shot_id, rel_path, bytes, sha256)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (id, shot_id, rel_path, bytes, sha256),
        )?;
        Ok(())
    }

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

    // --------------------------------------------------------------- checks

    pub fn add_check(
        &self,
        shot_id: &str,
        name: &str,
        status: &str,
        detail: Option<&str>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO checks (shot_id, name, status, detail)
             VALUES (?1, ?2, ?3, ?4)",
            (shot_id, name, status, detail),
        )?;
        Ok(())
    }

    // -------------------------------------------------------------- derived

    pub fn add_derived(
        &self,
        shot_id: &str,
        staged_path: &str,
        sha256: &str,
        bytes: u64,
        width: u32,
        height: u32,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO derived (shot_id, staged_path, sha256, bytes, width, height)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (shot_id, staged_path, sha256, bytes, width, height),
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ publishes

    pub fn add_publish(&self, shot_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO publishes (shot_id, state, attempts) VALUES (?1, 'pending', 0)",
            [shot_id],
        )?;
        Ok(())
    }

    pub fn update_publish_state(
        &self,
        shot_id: &str,
        state: &str,
        token: Option<&str>,
        media_item_id: Option<&str>,
        error: Option<&str>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE publishes
                SET state = ?2, upload_token = ?3, media_item_id = ?4, error = ?5,
                    attempts = attempts + 1
              WHERE shot_id = ?1",
            (shot_id, state, token, media_item_id, error),
        )?;
        Ok(())
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

    // ------------------------------------------------------------- settings

    // -----------------------------------------------------------------------
    // The GPS track library (`docs/geotag-plan.md`)
    // -----------------------------------------------------------------------

    /// Every imported track, most recent first.
    pub fn tracks(&self) -> SqlResult<Vec<TrackRow>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, source_path, creator, imported_at, point_count,
                    points_added, points_identical, points_conflicting,
                    first_fix, last_fix, min_lat, min_lon, max_lat, max_lon
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
                        first_fix, last_fix, min_lat, min_lon, max_lat, max_lon
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
                  first_fix, last_fix, min_lat, min_lon, max_lat, max_lon, gpx)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
            ],
        )?;

        for point in points {
            transaction.execute(
                "INSERT OR REPLACE INTO track_points (at, lat, lon, ele, track_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![point.at, point.lat, point.lon, point.ele, row.id],
            )?;
        }

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
    pub fn delete_track(&self, id: &str) -> SqlResult<usize> {
        let transaction = self.conn.unchecked_transaction()?;
        let removed = transaction.execute("DELETE FROM track_points WHERE track_id = ?1", [id])?;
        transaction.execute("DELETE FROM point_conflicts WHERE track_id = ?1", [id])?;
        transaction.execute("DELETE FROM tracks WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(removed)
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

    // ---------------------------------------------------------------- oauth

    /// Store an OAuth grant. The token must already be encrypted (§6.2 step 4)
    /// — this layer does not know how, deliberately, so a plaintext token
    /// cannot arrive here by a caller forgetting.
    pub fn set_oauth_grant(
        &self,
        provider: &str,
        encrypted_refresh_token: &str,
        scope: &str,
        connected_at: i64,
        state: &str,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO oauth
                 (provider, encrypted_refresh_token, scope, expires_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                provider,
                encrypted_refresh_token,
                scope,
                connected_at,
                state,
            ),
        )?;
        Ok(())
    }

    pub fn oauth_grant(&self, provider: &str) -> SqlResult<Option<OAuthGrant>> {
        self.conn
            .query_row(
                "SELECT encrypted_refresh_token, scope, expires_at, state
                   FROM oauth WHERE provider = ?1",
                [provider],
                |row| {
                    Ok(OAuthGrant {
                        encrypted_refresh_token: row.get(0)?,
                        scope: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        expires_at: row.get::<_, Option<i64>>(2)?.unwrap_or_default(),
                        state: row
                            .get::<_, Option<String>>(3)?
                            .unwrap_or_else(|| "connected".into()),
                    })
                },
            )
            .optional()
    }

    pub fn set_oauth_state(&self, provider: &str, state: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE oauth SET state = ?2 WHERE provider = ?1",
            (provider, state),
        )?;
        Ok(())
    }

    pub fn delete_oauth(&self, provider: &str) -> SqlResult<()> {
        self.conn
            .execute("DELETE FROM oauth WHERE provider = ?1", [provider])?;
        Ok(())
    }

    // ---------------------------------------------------- F15: publish state

    /// Record a shot as awaiting publication, without disturbing one that is
    /// already under way.
    ///
    /// `INSERT OR IGNORE`, because re-running a publish must resume from what is
    /// recorded rather than reset it to `pending` — resetting is precisely how a
    /// photograph gets uploaded and created twice (§6.3).
    pub fn queue_publish(
        &self,
        shot_id: &str,
        session_id: &str,
        stem: &str,
        source_sha256: &str,
        file_name: &str,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO publishes
                 (shot_id, session_id, stem, source_sha256, file_name, state, attempts)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0)",
            (shot_id, session_id, stem, source_sha256, file_name),
        )?;
        Ok(())
    }

    /// Every queued shot for one session, in a stable order.
    pub fn publishes_for_session(&self, session_id: &str) -> SqlResult<Vec<PublishRow>> {
        let mut statement = self.conn.prepare(
            "SELECT shot_id, stem, source_sha256, file_name, upload_token,
                    media_item_id, state, attempts, error
               FROM publishes WHERE session_id = ?1 ORDER BY stem",
        )?;
        let rows = statement.query_map([session_id], |row| {
            Ok(PublishRow {
                shot_id: row.get(0)?,
                stem: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                source_sha256: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                file_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                upload_token: row.get(4)?,
                media_item_id: row.get(5)?,
                state: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                attempts: row.get::<_, Option<i64>>(7)?.unwrap_or_default(),
                error: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn publish_row(&self, shot_id: &str) -> SqlResult<Option<PublishRow>> {
        Ok(self
            .publishes_matching("shot_id = ?1", shot_id)?
            .into_iter()
            .next())
    }

    fn publishes_matching(&self, predicate: &str, value: &str) -> SqlResult<Vec<PublishRow>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT shot_id, stem, source_sha256, file_name, upload_token,
                    media_item_id, state, attempts, error
               FROM publishes WHERE {predicate}"
        ))?;
        let rows = statement.query_map([value], |row| {
            Ok(PublishRow {
                shot_id: row.get(0)?,
                stem: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                source_sha256: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                file_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                upload_token: row.get(4)?,
                media_item_id: row.get(5)?,
                state: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                attempts: row.get::<_, Option<i64>>(7)?.unwrap_or_default(),
                error: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// Move a shot to `uploaded`, holding the token Google gave back (§6.3).
    pub fn record_upload(&self, shot_id: &str, upload_token: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE publishes SET state = 'uploaded', upload_token = ?2, error = NULL
              WHERE shot_id = ?1",
            (shot_id, upload_token),
        )?;
        Ok(())
    }

    /// Mark that a create is about to be sent.
    ///
    /// Written **before** the request, so a process that dies mid-call leaves a
    /// row saying so. `batchCreate` is not idempotent and the API cannot delete,
    /// so "we sent it and never heard back" has to be distinguishable from "we
    /// never sent it" — see `publish::Publisher`.
    pub fn record_creating(&self, shot_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE publishes SET state = 'creating', attempts = attempts + 1
              WHERE shot_id = ?1",
            [shot_id],
        )?;
        Ok(())
    }

    /// Record a media item **and** F16's published-hash entry in one
    /// transaction.
    ///
    /// Two writes that must not come apart. A crash between them leaves a
    /// photograph that is in Google Photos but not in the deduplication ledger,
    /// and the next ingest of that card publishes it a second time — the exact
    /// duplicate F16 exists to prevent, arriving through the back door.
    pub fn record_created_and_published(
        &self,
        shot_id: &str,
        media_item_id: &str,
        source_sha256: &str,
        stem: &str,
        derived_file_name: &str,
    ) -> SqlResult<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "UPDATE publishes SET state = 'created', media_item_id = ?2, error = NULL
              WHERE shot_id = ?1",
            (shot_id, media_item_id),
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO published
                 (source_sha256, stem, derived_sha256, session_id, media_item_id, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                source_sha256,
                stem,
                derived_file_name,
                shot_id,
                media_item_id,
                chrono::Utc::now().timestamp(),
            ),
        )?;

        tx.commit()
    }

    pub fn record_created(&self, shot_id: &str, media_item_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE publishes SET state = 'created', media_item_id = ?2, error = NULL
              WHERE shot_id = ?1",
            (shot_id, media_item_id),
        )?;
        Ok(())
    }

    /// A definite failure: nothing was created, so the shot goes back to the
    /// last state that is safe to resume from.
    pub fn record_publish_failure(&self, shot_id: &str, state: &str, error: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE publishes SET state = ?2, error = ?3 WHERE shot_id = ?1",
            (shot_id, state, error),
        )?;
        Ok(())
    }

    // ----------------------------------------------------------- dry runs

    /// Record that a dry run was performed for a session (§9.2 rule 3).
    pub fn record_dry_run(&self, session_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET dry_run_at = ?2 WHERE id = ?1",
            (session_id, chrono::Utc::now().timestamp()),
        )?;
        Ok(())
    }

    pub fn dry_run_at(&self, session_id: &str) -> SqlResult<Option<i64>> {
        self.conn
            .query_row(
                "SELECT dry_run_at FROM sessions WHERE id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
    }

    // ------------------------------------------------- F16: published hashes

    /// Record that one photograph reached Google Photos.
    ///
    /// `media_item_id` is optional because Phase 12's state machine reaches
    /// `uploaded` before it reaches `created`; a row with no media item is a
    /// photograph whose bytes are up but whose item is not yet made.
    pub fn record_published(
        &self,
        source_sha256: &str,
        stem: &str,
        derived_sha256: &str,
        session_id: &str,
        media_item_id: Option<&str>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO published
                 (source_sha256, stem, derived_sha256, session_id, media_item_id, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                source_sha256,
                stem,
                derived_sha256,
                session_id,
                media_item_id,
                chrono::Utc::now().timestamp(),
            ),
        )?;
        Ok(())
    }

    pub fn is_published(&self, source_sha256: &str) -> SqlResult<bool> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM published WHERE source_sha256 = ?1",
                [source_sha256],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Which of `hashes` have been published (F16).
    ///
    /// One query per chunk rather than one per hash, because a 400-frame card
    /// would otherwise make 400 round trips to answer a question SQLite can
    /// answer in one. Chunked at 500 because SQLite's default limit is 999
    /// bound parameters, and a manifest is not bounded by anything the caller
    /// controls — a burst-mode card can carry thousands of frames.
    pub fn published_among(
        &self,
        hashes: &[String],
    ) -> SqlResult<std::collections::HashSet<String>> {
        const CHUNK: usize = 500;
        let mut found = std::collections::HashSet::new();

        for chunk in hashes.chunks(CHUNK) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT source_sha256 FROM published WHERE source_sha256 IN ({placeholders})"
            );

            let mut statement = self.conn.prepare(&sql)?;
            let rows = statement.query_map(rusqlite::params_from_iter(chunk), |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                found.insert(row?);
            }
        }

        Ok(found)
    }

    // ------------------------------------------------------ handoff sessions

    /// Open a handoff session, storing the manifest it was opened with.
    ///
    /// The manifest is kept whole, as the JSON it arrived as. The protocol
    /// spans two requests — `sessions` then `ready` — and the second cannot
    /// verify arrivals without what the first was promised. Storing it verbatim
    /// also means a server restarted between the two still knows what it agreed
    /// to (F17).
    pub fn open_session(
        &self,
        id: &str,
        card_id: &str,
        manifest_json: &str,
        plan_json: &str,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (id, card_id, state, created_at, manifest, plan)
             VALUES (?1, ?2, 'open', ?3, ?4, ?5)",
            (
                id,
                card_id,
                chrono::Utc::now().timestamp(),
                manifest_json,
                plan_json,
            ),
        )?;
        Ok(())
    }

    /// The manifest and plan a session was opened with, as stored JSON.
    ///
    /// Both, together, because verification needs both and reading them in one
    /// statement means a session cannot be seen half-updated.
    pub fn session_agreement(&self, id: &str) -> SqlResult<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT manifest, plan FROM sessions WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    pub fn set_session_plan(&self, id: &str, plan_json: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET plan = ?2 WHERE id = ?1",
            (id, plan_json),
        )?;
        Ok(())
    }

    /// Store what verification found, so `GET .../shots` can answer after the
    /// job that produced it has finished.
    pub fn set_session_report(&self, id: &str, state: &str, report_json: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET state = ?2, report = ?3 WHERE id = ?1",
            (id, state, report_json),
        )?;
        Ok(())
    }

    pub fn session_report(&self, id: &str) -> SqlResult<Option<String>> {
        self.conn
            .query_row("SELECT report FROM sessions WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map(Option::flatten)
    }

    pub fn set_session_state(&self, id: &str, state: &str) -> SqlResult<()> {
        self.conn
            .execute("UPDATE sessions SET state = ?2 WHERE id = ?1", (id, state))?;
        Ok(())
    }

    pub fn session_state(&self, id: &str) -> SqlResult<Option<String>> {
        self.conn
            .query_row("SELECT state FROM sessions WHERE id = ?1", [id], |row| {
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

    /// A database that predates the track library gains it, and the schema it
    /// ends up with is the one a fresh database is created with.
    ///
    /// Comparing the whole of `sqlite_master` rather than looking for one
    /// table: a migration that adds a table but forgets its index leaves two
    /// databases that behave differently under load and identically under a
    /// test that only checks the table exists.
    #[test]
    fn a_database_from_before_the_track_library_ends_up_with_the_same_schema_as_a_fresh_one() {
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

        // A database as it stood before migration 7 was written.
        let old = dir.path().join("old.sqlite3");
        {
            let conn = Connection::open(&old).unwrap();
            for migration in &MIGRATIONS[..6] {
                conn.execute_batch(migration).unwrap();
            }
            conn.execute_batch("PRAGMA user_version = 6").unwrap();
        }

        let migrated = Ledger::open(&old).unwrap();
        let fresh = Ledger::open(dir.path().join("fresh.sqlite3")).unwrap();

        assert_eq!(migrated.schema_version().unwrap(), MIGRATIONS.len() as i64);
        assert_eq!(schema_of(&migrated), schema_of(&fresh));

        // And the new tables work in the migrated one, not merely exist.
        assert_eq!(migrated.tracks().unwrap(), vec![]);
        assert_eq!(migrated.points_between(0, i64::MAX).unwrap(), vec![]);
    }

    /// Phase 1 acceptance: a round-trip test per table. All ten tables of
    /// specification §7, every column written and read back.
    #[test]
    fn every_table_round_trips() {
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
        ledger.add_shot("shot-1", "card-1", "IMG_1234").unwrap();
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

        // checks
        ledger
            .add_check("shot-1", "resolution", "fail", Some("24.0 MP > 10 MP"))
            .unwrap();
        let (status, detail): (String, String) = conn
            .query_row(
                "SELECT status, detail FROM checks WHERE shot_id = 'shot-1' AND name = 'resolution'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "fail");
        assert_eq!(detail, "24.0 MP > 10 MP");

        // derived
        ledger
            .add_derived(
                "shot-1",
                "/staging/IMG_1234.jpg",
                "sha-derived",
                2048,
                3873,
                2582,
            )
            .unwrap();
        let (path, sha, bytes, w, h): (String, String, i64, u32, u32) = conn
            .query_row(
                "SELECT staged_path, sha256, bytes, width, height
                   FROM derived WHERE shot_id = 'shot-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(path, "/staging/IMG_1234.jpg");
        assert_eq!(sha, "sha-derived");
        assert_eq!(bytes, 2048);
        assert_eq!((w, h), (3873, 2582));

        // publishes — including the attempts counter
        ledger.add_publish("shot-1").unwrap();
        ledger
            .update_publish_state("shot-1", "uploaded", Some("upload-token"), None, None)
            .unwrap();
        ledger
            .update_publish_state(
                "shot-1",
                "created",
                Some("upload-token"),
                Some("media-item-1"),
                None,
            )
            .unwrap();
        let (state, token, item, attempts): (String, String, String, i64) = conn
            .query_row(
                "SELECT state, upload_token, media_item_id, attempts
                   FROM publishes WHERE shot_id = 'shot-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(state, "created");
        assert_eq!(token, "upload-token");
        assert_eq!(item, "media-item-1");
        assert_eq!(attempts, 2, "each state change counts an attempt");

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

        // oauth
        ledger
            .set_oauth_grant(
                "google",
                "cipher-text",
                "photoslibrary.appendonly",
                4102444800,
                "connected",
            )
            .unwrap();
        let grant = ledger.oauth_grant("google").unwrap().unwrap();
        assert_eq!(grant.encrypted_refresh_token, "cipher-text");
        assert_eq!(grant.scope, "photoslibrary.appendonly");
        assert_eq!(grant.expires_at, 4102444800);
        assert_eq!(grant.state, "connected");

        ledger.set_oauth_state("google", "disconnected").unwrap();
        assert_eq!(
            ledger.oauth_grant("google").unwrap().unwrap().state,
            "disconnected"
        );

        ledger.delete_oauth("google").unwrap();
        assert!(ledger.oauth_grant("google").unwrap().is_none());
    }

    #[test]
    fn reinserting_a_card_preserves_first_seen_and_keeps_its_label() {
        let ledger = Ledger::open_in_memory().unwrap();
        ledger
            .upsert_card("card-1", Some("EOS_DIGITAL"), "fp")
            .unwrap();
        let first: i64 = ledger
            .inner()
            .query_row(
                "SELECT first_seen FROM cards WHERE id = 'card-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // A later sighting that does not know the label must not erase it.
        ledger.add_card("card-1", "fp").unwrap();

        let (label, first_again): (String, i64) = ledger
            .inner()
            .query_row(
                "SELECT volume_label, first_seen FROM cards WHERE id = 'card-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(label, "EOS_DIGITAL");
        assert_eq!(first_again, first);
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
}
