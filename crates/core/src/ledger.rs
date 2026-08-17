//! SQLite persistence

use crate::jobs::{Job, JobStatus};
use rusqlite::{Connection, OptionalExtension, Result as SqlResult};
use std::path::Path;

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
];

pub struct Ledger {
    conn: Connection,
}

impl Ledger {
    pub fn open<P: AsRef<Path>>(path: P) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
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
    pub fn finish_job(&self, id: &str, status: JobStatus, error: Option<&str>) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE jobs
                SET state = ?2, error = ?3, finished_at = strftime('%s', 'now')
              WHERE id = ?1",
            (id, status.as_str(), error),
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> SqlResult<Option<Job>> {
        self.conn
            .query_row(
                "SELECT id, kind, state, progress, total, started_at, finished_at, error
                   FROM jobs WHERE id = ?1",
                [id],
                Self::row_to_job,
            )
            .optional()
    }

    pub fn jobs_with_status(&self, status: JobStatus) -> SqlResult<Vec<Job>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, state, progress, total, started_at, finished_at, error
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
        })
    }

    // ------------------------------------------------------------- settings

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

    pub fn set_oauth_token(
        &self,
        provider: &str,
        token: &str,
        scope: &str,
        expires_at: i64,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO oauth (provider, encrypted_refresh_token, scope, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            (provider, token, scope, expires_at),
        )?;
        Ok(())
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
            .finish_job("job-1", JobStatus::Completed, None)
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
            .set_oauth_token(
                "google",
                "cipher-text",
                "photoslibrary.appendonly",
                4102444800,
            )
            .unwrap();
        let (token, scope, exp): (String, String, i64) = conn
            .query_row(
                "SELECT encrypted_refresh_token, scope, expires_at
                   FROM oauth WHERE provider = 'google'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(token, "cipher-text");
        assert_eq!(scope, "photoslibrary.appendonly");
        assert_eq!(exp, 4102444800);
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
                .finish_job("job-done", JobStatus::Completed, None)
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
}
