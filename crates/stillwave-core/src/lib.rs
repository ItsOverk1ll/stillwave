// SPDX-License-Identifier: GPL-3.0-only
//! Domain types, identifiers and the sync clock shared by every Stillwave crate.

pub mod hlc;
pub mod ids;
pub mod model;

pub use hlc::{Hlc, HlcClock};
pub use ids::*;
pub use model::*;

/// Version of this crate, shown on the app's home screen.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
