// SPDX-License-Identifier: GPL-3.0-only
//! Plugin packages and signatures.
//!
//! On disk a package is a directory:
//! `manifest.toml`, `plugin.wasm`, `signature.sig`.
//! The `.sig` file is two text lines: `signer = <64 hex>` (public key) and
//! `signature = <128 hex>`. The signature covers a domain-separated,
//! length-prefixed encoding of the manifest bytes and the wasm bytes, so neither
//! can be altered or swapped independently.

use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::error::PluginError;

const DOMAIN: &[u8] = b"stillwave-plugin-v1\0";

/// An unverified plugin as read from disk or the network.
#[derive(Debug, Clone)]
pub struct PluginPackage {
    pub manifest_bytes: Vec<u8>,
    pub wasm_bytes: Vec<u8>,
    pub signature: SignatureFile,
}

/// Who signed, and the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureFile {
    pub signer: [u8; 32],
    pub signature: [u8; 64],
}

/// The exact bytes that are signed.
#[must_use]
pub fn signed_message(manifest: &[u8], wasm: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(DOMAIN.len() + 16 + manifest.len() + wasm.len());
    m.extend_from_slice(DOMAIN);
    m.extend_from_slice(&(manifest.len() as u64).to_le_bytes());
    m.extend_from_slice(manifest);
    m.extend_from_slice(&(wasm.len() as u64).to_le_bytes());
    m.extend_from_slice(wasm);
    m
}

/// Signs a package with `key`.
#[must_use]
pub fn sign(manifest: &[u8], wasm: &[u8], key: &SigningKey) -> SignatureFile {
    SignatureFile {
        signer: key.verifying_key().to_bytes(),
        signature: key.sign(&signed_message(manifest, wasm)).to_bytes(),
    }
}

impl SignatureFile {
    #[must_use]
    pub fn to_text(&self) -> String {
        format!(
            "signer = {}\nsignature = {}\n",
            hex_encode(&self.signer),
            hex_encode(&self.signature)
        )
    }

    pub fn parse(text: &str) -> Result<Self, PluginError> {
        let (mut signer, mut signature) = (None, None);
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| PluginError::BadSignatureFile(format!("bad line {line:?}")))?;
            match k.trim() {
                "signer" => signer = Some(v.trim()),
                "signature" => signature = Some(v.trim()),
                other => {
                    return Err(PluginError::BadSignatureFile(format!(
                        "unknown key {other:?}"
                    )))
                }
            }
        }
        let missing = |what| PluginError::BadSignatureFile(format!("missing {what}"));
        Ok(Self {
            signer: hex_array(signer.ok_or_else(|| missing("signer"))?)?,
            signature: hex_array(signature.ok_or_else(|| missing("signature"))?)?,
        })
    }

    /// Checks the signature against the embedded signer key. Whether that key is
    /// *trusted* is decided separately (see [`crate::trust`]).
    pub fn verify(&self, manifest: &[u8], wasm: &[u8]) -> Result<(), PluginError> {
        let key = VerifyingKey::from_bytes(&self.signer).map_err(|_| PluginError::BadSignature)?;
        key.verify_strict(
            &signed_message(manifest, wasm),
            &Signature::from_bytes(&self.signature),
        )
        .map_err(|_| PluginError::BadSignature)
    }
}

impl PluginPackage {
    /// Reads `manifest.toml`, `plugin.wasm` and `signature.sig` from `dir`.
    pub fn load_dir(dir: &Path) -> Result<Self, PluginError> {
        let read = |name: &str| {
            std::fs::read(dir.join(name)).map_err(|e| PluginError::Io(format!("{name}: {e}")))
        };
        Ok(Self {
            manifest_bytes: read("manifest.toml")?,
            wasm_bytes: read("plugin.wasm")?,
            signature: SignatureFile::parse(&String::from_utf8_lossy(&read("signature.sig")?))?,
        })
    }
}

#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn hex_array<const N: usize>(s: &str) -> Result<[u8; N], PluginError> {
    let bad = || PluginError::BadSignatureFile(format!("expected {} hex characters", N * 2));
    if s.len() != N * 2 || !s.is_ascii() {
        return Err(bad());
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn sign_verify_round_trip_through_text() {
        let sig = sign(b"manifest", b"wasm", &key(1));
        let parsed = SignatureFile::parse(&sig.to_text()).unwrap();
        assert_eq!(parsed, sig);
        parsed.verify(b"manifest", b"wasm").unwrap();
    }

    #[test]
    fn tampering_with_either_part_fails() {
        let sig = sign(b"manifest", b"wasm", &key(1));
        assert!(sig.verify(b"manifesT", b"wasm").is_err());
        assert!(sig.verify(b"manifest", b"wasM").is_err());
    }

    #[test]
    fn parts_cannot_be_shifted_between_manifest_and_wasm() {
        let sig = sign(b"ab", b"c", &key(1));
        assert!(sig.verify(b"a", b"bc").is_err());
    }

    #[test]
    fn malformed_signature_files_are_rejected() {
        for text in [
            "",
            "signer = 00",
            "nonsense",
            "signer = zz\nsignature = 00",
            "x = 1",
        ] {
            assert!(SignatureFile::parse(text).is_err(), "{text:?}");
        }
    }
}
