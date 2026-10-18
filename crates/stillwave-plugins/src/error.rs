// SPDX-License-Identifier: GPL-3.0-only
use crate::manifest::{ManifestError, Permission, Tier};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    #[error("could not read plugin: {0}")]
    Io(String),
    #[error("bad signature file: {0}")]
    BadSignatureFile(String),
    #[error("signature does not match the plugin contents")]
    BadSignature,
    #[error("plugin is signed by a key that is not trusted")]
    UntrustedSigner,
    #[error("plugin claims tier {claimed:?} but is not signed by a key trusted for it")]
    TierNotSatisfied { claimed: Tier },
    #[error("plugin needs app version {required} or newer (this is {actual})")]
    AppTooOld { required: String, actual: String },
    #[error("plugin tried to use `{0}` without permission")]
    PermissionDenied(Permission),
    #[error("plugin runtime error: {0}")]
    Runtime(String),
    #[error("no WASM runtime is available in this build")]
    RuntimeUnavailable,
}
