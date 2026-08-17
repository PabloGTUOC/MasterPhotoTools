//! SQLite persistence

use rusqlite::{Connection, Result as SqlResult};
use std::path::Path;

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

    fn apply_migrations(conn: &Connection) -> SqlResult<()> {
        conn.execute_batch(
            "
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
            ",
        )?;
        Ok(())
    }

    pub fn inner(&self) -> &Connection {
        &self.conn
    }

    pub fn add_card(&self, id: &str, fingerprint: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO cards (id, fingerprint, first_seen) VALUES (?1, ?2, strftime('%s', 'now')) 
             ON CONFLICT(id) DO UPDATE SET last_seen = strftime('%s', 'now')",
            (id, fingerprint)
        )?;
        Ok(())
    }

    pub fn add_shot(&self, id: &str, card_id: &str, stem: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO shots (id, card_id, stem, status) VALUES (?1, ?2, ?3, 'new')",
            (id, card_id, stem),
        )?;
        Ok(())
    }

    pub fn add_asset(
        &self,
        id: &str,
        shot_id: &str,
        rel_path: &str,
        bytes: u64,
        sha256: &str,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO assets (id, shot_id, rel_path, bytes, sha256) VALUES (?1, ?2, ?3, ?4, ?5)",
            (id, shot_id, rel_path, bytes, sha256)
        )?;
        Ok(())
    }

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
            "INSERT OR REPLACE INTO derived (shot_id, staged_path, sha256, bytes, width, height) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (shot_id, staged_path, sha256, bytes, width, height)
        )?;
        Ok(())
    }

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
            "UPDATE publishes SET state = ?2, upload_token = ?3, media_item_id = ?4, error = ?5, attempts = attempts + 1 WHERE shot_id = ?1",
            (shot_id, state, token, media_item_id, error)
        )?;
        Ok(())
    }

    pub fn set_oauth_token(
        &self,
        provider: &str,
        token: &str,
        scope: &str,
        expires_at: i64,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO oauth (provider, encrypted_refresh_token, scope, expires_at) VALUES (?1, ?2, ?3, ?4)",
            (provider, token, scope, expires_at)
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_and_roundtrip() {
        let ledger = Ledger::open_in_memory().unwrap();
        let conn = ledger.inner();

        // Test users
        conn.execute(
            "INSERT INTO users (uid, display_name, added_at) VALUES (?1, ?2, ?3)",
            ("user1", "Pablo", 123456789),
        )
        .unwrap();
        let name: String = conn
            .query_row(
                "SELECT display_name FROM users WHERE uid = 'user1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Pablo");

        // Test settings
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            ("theme", "dark"),
        )
        .unwrap();
        let val: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'theme'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(val, "dark");

        // Verify the tables exist by inserting dummy rows without throwing errors
        conn.execute("INSERT INTO cards (id) VALUES ('card1')", [])
            .unwrap();
        conn.execute("INSERT INTO shots (id) VALUES ('shot1')", [])
            .unwrap();
        conn.execute("INSERT INTO assets (id) VALUES ('asset1')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO checks (shot_id, name) VALUES ('shot1', 'date')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO derived (shot_id) VALUES ('shot1')", [])
            .unwrap();
        conn.execute("INSERT INTO publishes (shot_id) VALUES ('shot1')", [])
            .unwrap();
        conn.execute("INSERT INTO jobs (id) VALUES ('job1')", [])
            .unwrap();
        conn.execute("INSERT INTO oauth (provider) VALUES ('google')", [])
            .unwrap();
    }
}
