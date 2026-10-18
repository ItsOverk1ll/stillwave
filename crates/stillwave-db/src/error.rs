// SPDX-License-Identifier: GPL-3.0-only

/// Errors from the database layer.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database holds invalid data: {0}")]
    Corrupt(String),
    #[error("database schema v{found} is newer than this app supports (v{supported})")]
    NewerSchema { found: u32, supported: u32 },
}
