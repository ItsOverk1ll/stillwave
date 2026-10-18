// SPDX-License-Identifier: GPL-3.0-only
//! SQLite persistence for Stillwave: schema, migrations, a small repository
//! API and full-text search. Every user-data write also appends a `change_log`
//! row in the same transaction so devices can sync later.

mod error;
mod migrations;
mod repo;

pub use error::DbError;
pub use migrations::{current_version, latest_version, migrate};
pub use repo::Db;
