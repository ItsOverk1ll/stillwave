// SPDX-License-Identifier: GPL-3.0-only
//! Developer tool: generate keys, sign and verify plugin directories.
//!
//! ```text
//! stillwave-plugin-tool keygen <name>          writes keys/<name>.key (secret) and keys/<name>.pub
//! stillwave-plugin-tool sign <dir> <key-file>  writes <dir>/signature.sig
//! stillwave-plugin-tool verify <dir> <pub-file> [official|community]
//! stillwave-plugin-tool run <dir> <pub-file> <export>   verify, load and call an export
//! ```
//!
//! `keys/` and `*.key` are git-ignored. Never commit a private key.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ed25519_dalek::SigningKey;
use semver::Version;
use stillwave_plugins::package::{hex_array, hex_encode, sign};
use stillwave_plugins::{
    generate_signing_key, verify, HostApi, Limits, PluginHost, PluginPackage, TrustedKeys,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["keygen", name] => keygen(name),
        ["sign", dir, key] => sign_dir(Path::new(dir), Path::new(key)),
        ["verify", dir, public] => verify_dir(Path::new(dir), Path::new(public), "official"),
        ["verify", dir, public, tier] => verify_dir(Path::new(dir), Path::new(public), tier),
        ["run", dir, public, export] => run_export(Path::new(dir), Path::new(public), export),
        _ => Err(
            "usage:\n  keygen <name>\n  sign <dir> <key-file>\n  verify <dir> <pub-file> [official|community]\n  run <dir> <pub-file> <export>"
                .into(),
        ),
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn keygen(name: &str) -> Result<String, String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("key name may contain only letters, digits, - and _".into());
    }
    let dir = PathBuf::from("keys");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (secret, public) = (
        dir.join(format!("{name}.key")),
        dir.join(format!("{name}.pub")),
    );
    if secret.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite",
            secret.display()
        ));
    }
    let key = generate_signing_key().map_err(|e| e.to_string())?;
    std::fs::write(&secret, hex_encode(&key.to_bytes())).map_err(|e| e.to_string())?;
    std::fs::write(&public, hex_encode(&key.verifying_key().to_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "wrote {} (SECRET, keep out of git) and {}",
        secret.display(),
        public.display()
    ))
}

fn read_hex32(path: &Path) -> Result<[u8; 32], String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    hex_array::<32>(text.trim()).map_err(|e| format!("{}: {e}", path.display()))
}

fn sign_dir(dir: &Path, key_file: &Path) -> Result<String, String> {
    let key = SigningKey::from_bytes(&read_hex32(key_file)?);
    let read = |n: &str| std::fs::read(dir.join(n)).map_err(|e| format!("{n}: {e}"));
    let sig = sign(&read("manifest.toml")?, &read("plugin.wasm")?, &key);
    std::fs::write(dir.join("signature.sig"), sig.to_text()).map_err(|e| e.to_string())?;
    Ok(format!("signed {}", dir.display()))
}

fn verify_dir(dir: &Path, pub_file: &Path, tier: &str) -> Result<String, String> {
    let public = read_hex32(pub_file)?;
    let trusted = match tier {
        "official" => TrustedKeys {
            official: vec![public],
            community: vec![],
        },
        "community" => TrustedKeys {
            official: vec![],
            community: vec![public],
        },
        other => return Err(format!("unknown tier {other:?}")),
    };
    let pkg = PluginPackage::load_dir(dir).map_err(|e| e.to_string())?;
    // Any app version: this tool checks authenticity, not compatibility.
    let v = verify(&pkg, &trusted, &Version::new(u64::MAX, 0, 0)).map_err(|e| e.to_string())?;
    Ok(format!(
        "ok: {} {} ({:?})",
        v.manifest.id, v.manifest.version, v.manifest.tier
    ))
}

/// Prints plugin output; reports a made-up library size so permissions can be tried.
struct Console;

impl HostApi for Console {
    fn log(&mut self, plugin_id: &str, message: &str) {
        println!("[{plugin_id}] {message}");
    }
    fn library_track_count(&mut self) -> i64 {
        7
    }
    fn create_playlist(&mut self, name: &str) -> bool {
        println!("(would create playlist {name:?})");
        true
    }
}

fn run_export(dir: &Path, pub_file: &Path, export: &str) -> Result<String, String> {
    let trusted = TrustedKeys {
        official: vec![read_hex32(pub_file)?],
        community: vec![],
    };
    let pkg = PluginPackage::load_dir(dir).map_err(|e| e.to_string())?;
    let plugin =
        verify(&pkg, &trusted, &Version::new(u64::MAX, 0, 0)).map_err(|e| e.to_string())?;
    let host = PluginHost::new(Limits::default()).map_err(|e| e.to_string())?;
    let mut loaded = host
        .load(&plugin, Box::new(Console))
        .map_err(|e| e.to_string())?;
    let value = loaded.call(export).map_err(|e| e.to_string())?;
    Ok(format!("{export}() returned {value}"))
}
