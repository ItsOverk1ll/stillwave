// SPDX-License-Identifier: GPL-3.0-only
//! Plugin manifest (TOML). TOML is human-written, allows comments, and is
//! already familiar from `Cargo.toml`. The signature covers the exact manifest
//! bytes, so the format has no bearing on security.
//!
//! ```toml
//! id = "org.stillwave.hello"
//! name = "Hello"
//! version = "0.1.0"
//! author = "Stillwave"
//! description = "Says hello."
//! min_app_version = "0.0.1"
//! tier = "official"            # or "community"
//! permissions = ["library.read", "network"]
//!
//! [network]                    # required iff "network" is requested
//! domains = ["api.jamendo.com"]
//! ```

use std::collections::BTreeSet;

use semver::Version;
use serde::{Deserialize, Serialize};

/// Everything a plugin may ask for. Anything not listed is denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Outbound HTTPS to the domains in `[network]` only.
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "microphone.clip")]
    MicrophoneClip,
    #[serde(rename = "clipboard.read")]
    ClipboardRead,
    #[serde(rename = "files.read_selected")]
    FilesReadSelected,
    #[serde(rename = "playlists.read")]
    PlaylistsRead,
    #[serde(rename = "playlists.write")]
    PlaylistsWrite,
    #[serde(rename = "library.read")]
    LibraryRead,
}

impl Permission {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::MicrophoneClip => "microphone.clip",
            Self::ClipboardRead => "clipboard.read",
            Self::FilesReadSelected => "files.read_selected",
            Self::PlaylistsRead => "playlists.read",
            Self::PlaylistsWrite => "playlists.write",
            Self::LibraryRead => "library.read",
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Made and signed by the Stillwave project.
    Official,
    /// Made by anyone; signed by its author.
    Community,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NetworkSection {
    #[serde(default)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub author: String,
    pub description: String,
    pub min_app_version: Version,
    pub tier: Tier,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub network: Option<NetworkSection>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("manifest is not valid TOML or has wrong fields: {0}")]
    Parse(String),
    #[error("invalid plugin id {0:?}: use lowercase dotted segments like `org.example.thing`")]
    BadId(String),
    #[error("`{0}` must not be empty")]
    Empty(&'static str),
    #[error("permission `{0}` is listed more than once")]
    DuplicatePermission(Permission),
    #[error("permission `network` needs a non-empty [network] domains list")]
    NetworkWithoutDomains,
    #[error("[network] domains given but permission `network` was not requested")]
    DomainsWithoutNetwork,
    #[error("invalid network domain {0:?}: use a bare lowercase hostname, no scheme, port, path or wildcard")]
    BadDomain(String),
}

impl Manifest {
    /// Parses and validates manifest text.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let m: Self = toml::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    /// Checks every rule that serde cannot express.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if !valid_id(&self.id) {
            return Err(ManifestError::BadId(self.id.clone()));
        }
        for (field, value) in [
            ("name", &self.name),
            ("author", &self.author),
            ("description", &self.description),
        ] {
            if value.trim().is_empty() {
                return Err(ManifestError::Empty(field));
            }
        }
        let mut seen = BTreeSet::new();
        for p in &self.permissions {
            if !seen.insert(*p) {
                return Err(ManifestError::DuplicatePermission(*p));
            }
        }
        let domains = self.network.as_ref().map_or(&[][..], |n| &n.domains[..]);
        match (self.has(Permission::Network), domains.is_empty()) {
            (true, true) => return Err(ManifestError::NetworkWithoutDomains),
            (false, false) => return Err(ManifestError::DomainsWithoutNetwork),
            _ => {}
        }
        if let Some(bad) = domains.iter().find(|d| !valid_domain(d)) {
            return Err(ManifestError::BadDomain(bad.clone()));
        }
        Ok(())
    }

    #[must_use]
    pub fn has(&self, p: Permission) -> bool {
        self.permissions.contains(&p)
    }

    /// Domains the plugin may contact (empty without the `network` permission).
    #[must_use]
    pub fn allowed_domains(&self) -> &[String] {
        self.network.as_ref().map_or(&[], |n| &n.domains)
    }
}

fn valid_id(id: &str) -> bool {
    let segments: Vec<&str> = id.split('.').collect();
    id.len() <= 64
        && segments.len() >= 2
        && segments.iter().all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !s.starts_with('-')
                && !s.ends_with('-')
        })
}

fn valid_domain(d: &str) -> bool {
    d.len() <= 253
        && d.contains('.')
        && d.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
id = "org.stillwave.hello"
name = "Hello"
version = "0.1.0"
author = "Stillwave"
description = "Says hello."
min_app_version = "0.0.1"
tier = "official"
"#;

    fn with(extra: &str) -> String {
        format!("{BASE}{extra}")
    }

    #[test]
    fn minimal_manifest_parses() {
        let m = Manifest::parse(BASE).unwrap();
        assert_eq!(m.id, "org.stillwave.hello");
        assert_eq!(m.tier, Tier::Official);
        assert!(m.permissions.is_empty());
    }

    #[test]
    fn all_permission_names_parse() {
        let m = Manifest::parse(&with(
            "permissions = [\"microphone.clip\",\"clipboard.read\",\"files.read_selected\",\"playlists.read\",\"playlists.write\",\"library.read\"]",
        ))
        .unwrap();
        assert_eq!(m.permissions.len(), 6);
        assert!(m.has(Permission::PlaylistsWrite));
    }

    #[test]
    fn network_needs_domains_and_vice_versa() {
        assert_eq!(
            Manifest::parse(&with("permissions = [\"network\"]")),
            Err(ManifestError::NetworkWithoutDomains)
        );
        assert_eq!(
            Manifest::parse(&with("[network]\ndomains = [\"example.com\"]")),
            Err(ManifestError::DomainsWithoutNetwork)
        );
        let ok = Manifest::parse(&with(
            "permissions = [\"network\"]\n[network]\ndomains = [\"api.jamendo.com\"]",
        ))
        .unwrap();
        assert_eq!(ok.allowed_domains(), ["api.jamendo.com"]);
    }

    #[test]
    fn bad_domains_are_rejected() {
        for bad in [
            "https://x.com",
            "*.x.com",
            "x.com/path",
            "x.com:443",
            "localhost",
            "X.com",
        ] {
            let text = with(&format!(
                "permissions = [\"network\"]\n[network]\ndomains = [\"{bad}\"]"
            ));
            assert_eq!(
                Manifest::parse(&text),
                Err(ManifestError::BadDomain(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn unknown_permission_or_field_is_an_error() {
        assert!(matches!(
            Manifest::parse(&with("permissions = [\"root.everything\"]")),
            Err(ManifestError::Parse(_))
        ));
        assert!(matches!(
            Manifest::parse(&with("surprise = 1")),
            Err(ManifestError::Parse(_))
        ));
    }

    #[test]
    fn duplicate_permissions_are_rejected() {
        assert_eq!(
            Manifest::parse(&with("permissions = [\"library.read\",\"library.read\"]")),
            Err(ManifestError::DuplicatePermission(Permission::LibraryRead))
        );
    }

    #[test]
    fn ids_are_validated() {
        for bad in ["hello", "Org.Hello", "org..x", "org.-x", "org.x y", ""] {
            let text = BASE.replace("org.stillwave.hello", bad);
            assert!(Manifest::parse(&text).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn bad_version_and_empty_fields_are_errors() {
        assert!(Manifest::parse(&BASE.replace("0.1.0", "one")).is_err());
        assert_eq!(
            Manifest::parse(&BASE.replace("Says hello.", "  ")),
            Err(ManifestError::Empty("description"))
        );
    }

    #[test]
    fn errors_are_readable() {
        let e = Manifest::parse(&with("permissions = [\"network\"]")).unwrap_err();
        assert!(e.to_string().contains("[network] domains"));
    }
}
