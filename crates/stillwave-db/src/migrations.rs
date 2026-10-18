// SPDX-License-Identifier: GPL-3.0-only
//! Versioned schema migrations. Each runs in its own transaction at startup;
//! `PRAGMA user_version` records the last one applied. Never edit a released
//! migration: add a new one.

use rusqlite::Connection;

use crate::error::DbError;

pub(crate) const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_library_lookups.sql"),
];

/// The schema version a fully migrated database reports.
#[must_use]
pub fn latest_version() -> u32 {
    u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX)
}

/// The version currently recorded in the database.
pub fn current_version(conn: &Connection) -> Result<u32, DbError> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Applies every migration newer than the database's version.
pub fn migrate(conn: &mut Connection) -> Result<(), DbError> {
    let found = current_version(conn)?;
    let supported = latest_version();
    if found > supported {
        return Err(DbError::NewerSchema { found, supported });
    }
    for (idx, sql) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        let version = idx + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}
