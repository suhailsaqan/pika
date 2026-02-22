use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::ProfileCache;

pub fn open_profile_db(data_dir: &str) -> Result<Connection, rusqlite::Error> {
    let path = std::path::Path::new(data_dir).join("profiles.sqlite3");
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS profiles (
            pubkey TEXT PRIMARY KEY,
            metadata JSONB,
            name TEXT,
            about TEXT,
            picture_url TEXT,
            event_created_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS my_devices (
            fingerprint TEXT PRIMARY KEY,
            key_package_event_id TEXT NOT NULL,
            key_package_event_json TEXT NOT NULL,
            published_at INTEGER NOT NULL,
            is_current_device INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS rejected_devices (
            fingerprint TEXT PRIMARY KEY
        );",
    )?;
    Ok(conn)
}

pub fn load_profiles(conn: &Connection) -> HashMap<String, ProfileCache> {
    let mut map = HashMap::new();
    let mut stmt = match conn.prepare(
        "SELECT pubkey, display_name, name, about, picture_url, event_created_at FROM profiles",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(%e, "failed to prepare profile load query");
            return map;
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, i64>(5)?,
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%e, "failed to query profiles from cache db");
            return map;
        }
    };
    for row in rows.flatten() {
        let (pubkey, display_name, name, about, picture_url, event_created_at) = row;

        map.insert(
            pubkey,
            ProfileCache {
                metadata_json: None,
                name: display_name.or(name.clone()),
                username: name,
                about,
                picture_url,
                event_created_at,
                last_checked_at: 0, // always re-check on app launch
            },
        );
    }
    map
}

/// Load the full metadata JSON for a single profile (used for profile editing).
pub fn load_metadata_json(conn: &Connection, pubkey: &str) -> Option<String> {
    conn.query_row(
        "SELECT json(metadata) FROM profiles WHERE pubkey = ?1",
        [pubkey],
        |row| row.get(0),
    )
    .ok()
}

pub fn save_profile(conn: &Connection, pubkey: &str, cache: &ProfileCache) {
    if let Err(e) = conn.execute(
        "INSERT INTO profiles (pubkey, metadata, name, about, picture_url, event_created_at)
         VALUES (?1, jsonb(?2), ?3, ?4, ?5, ?6)
         ON CONFLICT(pubkey) DO UPDATE SET
            metadata = jsonb(excluded.metadata),
            name = excluded.name,
            about = excluded.about,
            picture_url = excluded.picture_url,
            event_created_at = excluded.event_created_at",
        rusqlite::params![
            pubkey,
            cache.metadata_json,
            cache.name,
            cache.about,
            cache.picture_url,
            cache.event_created_at,
        ],
    ) {
        tracing::warn!(%e, pubkey, "failed to save profile to cache db");
    }
}

/// Delete all cached profiles (used on logout).
pub fn clear_all(conn: &Connection) {
    if let Err(e) = conn.execute_batch("DELETE FROM profiles;") {
        tracing::warn!(%e, "failed to clear profile cache db");
    }
}

// ── Device cache ─────────────────────────────────────────────────────

pub fn load_devices(conn: &Connection) -> Vec<crate::state::DeviceInfo> {
    let mut stmt = match conn.prepare(
        "SELECT fingerprint, key_package_event_id, key_package_event_json, published_at, is_current_device
         FROM my_devices ORDER BY is_current_device DESC, published_at DESC",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(%e, "failed to prepare device load query");
            return vec![];
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok(crate::state::DeviceInfo {
            fingerprint: row.get(0)?,
            key_package_event_id: row.get(1)?,
            key_package_event_json: row.get(2)?,
            published_at: row.get(3)?,
            is_current_device: row.get::<_, i32>(4)? != 0,
        })
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%e, "failed to query devices from cache db");
            return vec![];
        }
    };
    rows.flatten().collect()
}

pub fn load_rejected_fingerprints(conn: &Connection) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut stmt = match conn.prepare("SELECT fingerprint FROM rejected_devices") {
        Ok(s) => s,
        Err(_) => return set,
    };
    let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
        Ok(r) => r,
        Err(_) => return set,
    };
    for fp in rows.flatten() {
        set.insert(fp);
    }
    set
}

pub fn add_rejected_fingerprint(conn: &Connection, fingerprint: &str) {
    let _ = conn.execute(
        "INSERT OR IGNORE INTO rejected_devices (fingerprint) VALUES (?1)",
        [fingerprint],
    );
}

pub fn save_devices(conn: &Connection, devices: &[crate::state::DeviceInfo]) {
    if let Err(e) = conn.execute("DELETE FROM my_devices", []) {
        tracing::warn!(%e, "failed to clear device cache");
        return;
    }
    for d in devices {
        if let Err(e) = conn.execute(
            "INSERT INTO my_devices (fingerprint, key_package_event_id, key_package_event_json, published_at, is_current_device)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                d.fingerprint,
                d.key_package_event_id,
                d.key_package_event_json,
                d.published_at,
                d.is_current_device as i32,
            ],
        ) {
            tracing::warn!(%e, fingerprint = d.fingerprint, "failed to save device to cache db");
        }
    }
}
