// SPDX-License-Identifier: GPL-3.0-only
//! Decides whether a package may be loaded: valid manifest, trusted signature,
//! tier satisfied, app new enough.

use semver::Version;

use crate::error::PluginError;
use crate::manifest::{Manifest, Tier};
use crate::package::PluginPackage;

/// Public keys this app trusts.
#[derive(Debug, Clone, Default)]
pub struct TrustedKeys {
    /// Stillwave project keys. Required for `tier = "official"`.
    pub official: Vec<[u8; 32]>,
    /// Author keys accepted for `tier = "community"` (later: from the registry).
    pub community: Vec<[u8; 32]>,
}

/// A package whose signature and manifest have passed every check.
#[derive(Debug, Clone)]
pub struct VerifiedPlugin {
    pub manifest: Manifest,
    pub wasm_bytes: Vec<u8>,
}

/// Verifies `pkg` for an app at `app_version`.
///
/// Order matters: the signature is checked *before* the manifest is trusted for
/// anything, and the tier a manifest claims is never taken on its word.
pub fn verify(
    pkg: &PluginPackage,
    trusted: &TrustedKeys,
    app_version: &Version,
) -> Result<VerifiedPlugin, PluginError> {
    let signer = &pkg.signature.signer;
    let official = trusted.official.contains(signer);
    let community = trusted.community.contains(signer);
    if !official && !community {
        return Err(PluginError::UntrustedSigner);
    }
    pkg.signature.verify(&pkg.manifest_bytes, &pkg.wasm_bytes)?;

    let text = std::str::from_utf8(&pkg.manifest_bytes)
        .map_err(|e| PluginError::Io(format!("manifest is not UTF-8: {e}")))?;
    let manifest = Manifest::parse(text)?;

    match manifest.tier {
        Tier::Official if !official => {
            return Err(PluginError::TierNotSatisfied {
                claimed: Tier::Official,
            })
        }
        // Official keys may also sign community-tier plugins.
        Tier::Official | Tier::Community => {}
    }
    if &manifest.min_app_version > app_version {
        return Err(PluginError::AppTooOld {
            required: manifest.min_app_version.to_string(),
            actual: app_version.to_string(),
        });
    }
    Ok(VerifiedPlugin {
        manifest,
        wasm_bytes: pkg.wasm_bytes.clone(),
    })
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::package::sign;

    const MANIFEST: &str = r#"
id = "org.stillwave.hello"
name = "Hello"
version = "0.1.0"
author = "Stillwave"
description = "Says hello."
min_app_version = "0.1.0"
tier = "official"
"#;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn pkg(manifest: &str, wasm: &[u8], k: &SigningKey) -> PluginPackage {
        PluginPackage {
            manifest_bytes: manifest.as_bytes().to_vec(),
            wasm_bytes: wasm.to_vec(),
            signature: sign(manifest.as_bytes(), wasm, k),
        }
    }

    fn trust(official: &SigningKey, community: &SigningKey) -> TrustedKeys {
        TrustedKeys {
            official: vec![official.verifying_key().to_bytes()],
            community: vec![community.verifying_key().to_bytes()],
        }
    }

    fn app() -> Version {
        Version::new(0, 1, 0)
    }

    #[test]
    fn valid_official_plugin_is_accepted() {
        let (off, com) = (key(1), key(2));
        let v = verify(&pkg(MANIFEST, b"\0asm", &off), &trust(&off, &com), &app()).unwrap();
        assert_eq!(v.manifest.id, "org.stillwave.hello");
    }

    #[test]
    fn tampered_wasm_is_rejected() {
        let (off, com) = (key(1), key(2));
        let mut p = pkg(MANIFEST, b"\0asm", &off);
        p.wasm_bytes.push(0);
        assert!(matches!(
            verify(&p, &trust(&off, &com), &app()),
            Err(PluginError::BadSignature)
        ));
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let (off, com) = (key(1), key(2));
        let mut p = pkg(MANIFEST, b"\0asm", &off);
        p.manifest_bytes = MANIFEST
            .replace(
                "tier = \"official\"",
                "tier = \"official\"\npermissions = [\"playlists.write\"]",
            )
            .into_bytes();
        assert!(matches!(
            verify(&p, &trust(&off, &com), &app()),
            Err(PluginError::BadSignature)
        ));
    }

    #[test]
    fn unknown_signer_is_rejected() {
        let (off, com, stranger) = (key(1), key(2), key(3));
        assert!(matches!(
            verify(
                &pkg(MANIFEST, b"\0asm", &stranger),
                &trust(&off, &com),
                &app()
            ),
            Err(PluginError::UntrustedSigner)
        ));
    }

    #[test]
    fn signature_made_with_a_different_key_than_claimed_is_rejected() {
        let (off, com, stranger) = (key(1), key(2), key(3));
        let mut p = pkg(MANIFEST, b"\0asm", &stranger);
        p.signature.signer = off.verifying_key().to_bytes(); // forge the signer field
        assert!(matches!(
            verify(&p, &trust(&off, &com), &app()),
            Err(PluginError::BadSignature)
        ));
    }

    #[test]
    fn community_plugin_cannot_claim_official() {
        let (off, com) = (key(1), key(2));
        // Properly signed by a trusted *community* key, but the manifest says official.
        assert!(matches!(
            verify(&pkg(MANIFEST, b"\0asm", &com), &trust(&off, &com), &app()),
            Err(PluginError::TierNotSatisfied {
                claimed: Tier::Official
            })
        ));
    }

    #[test]
    fn community_plugin_signed_by_community_key_is_accepted() {
        let (off, com) = (key(1), key(2));
        let manifest = MANIFEST.replace("official", "community");
        assert!(verify(&pkg(&manifest, b"\0asm", &com), &trust(&off, &com), &app()).is_ok());
    }

    #[test]
    fn too_old_app_is_rejected() {
        let (off, com) = (key(1), key(2));
        let r = verify(
            &pkg(MANIFEST, b"\0asm", &off),
            &trust(&off, &com),
            &Version::new(0, 0, 9),
        );
        assert!(matches!(r, Err(PluginError::AppTooOld { .. })));
    }
}
