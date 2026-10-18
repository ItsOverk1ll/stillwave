// SPDX-License-Identifier: GPL-3.0-only
#![cfg(feature = "wasmtime-host")]

use std::sync::{Arc, Mutex};

use semver::Version;
use stillwave_plugins::package::sign;
use stillwave_plugins::{
    generate_signing_key, verify, HostApi, Limits, Permission, PluginError, PluginHost,
    PluginPackage, TrustedKeys, VerifiedPlugin,
};

const WAT: &str = r#"
(module
  (import "stillwave" "log" (func $log (param i32 i32)))
  (import "stillwave" "library_track_count" (func $count (result i64)))
  (import "stillwave" "playlist_create" (func $mk (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "hi")
  (func (export "hello") (result i32) i32.const 0 i32.const 2 call $log i32.const 42)
  (func (export "count") (result i32) call $count i32.wrap_i64)
  (func (export "mkplaylist") (result i32) i32.const 0 i32.const 2 call $mk)
  (func (export "spin") (result i32) (loop $l (br $l)) i32.const 0)
  (func (export "bad_ptr") (result i32) i32.const 70000 i32.const 8 call $log i32.const 1))
"#;

fn manifest(permissions: &str) -> String {
    format!(
        r#"id = "org.stillwave.hello"
name = "Hello"
version = "0.1.0"
author = "Stillwave"
description = "Test plugin."
min_app_version = "0.0.1"
tier = "official"
permissions = [{permissions}]
"#
    )
}

#[derive(Default, Clone)]
struct Recorder {
    logs: Arc<Mutex<Vec<String>>>,
    playlists: Arc<Mutex<Vec<String>>>,
}

impl HostApi for Recorder {
    fn log(&mut self, plugin_id: &str, message: &str) {
        self.logs
            .lock()
            .unwrap()
            .push(format!("{plugin_id}: {message}"));
    }
    fn library_track_count(&mut self) -> i64 {
        1234
    }
    fn create_playlist(&mut self, name: &str) -> bool {
        self.playlists.lock().unwrap().push(name.into());
        true
    }
}

fn verified(wat: &str, permissions: &str) -> VerifiedPlugin {
    let wasm = wat::parse_str(wat).unwrap();
    let manifest = manifest(permissions);
    let key = generate_signing_key().unwrap();
    let pkg = PluginPackage {
        manifest_bytes: manifest.clone().into_bytes(),
        wasm_bytes: wasm.clone(),
        signature: sign(manifest.as_bytes(), &wasm, &key),
    };
    let trusted = TrustedKeys {
        official: vec![key.verifying_key().to_bytes()],
        community: vec![],
    };
    verify(&pkg, &trusted, &Version::new(0, 1, 0)).unwrap()
}

fn load(permissions: &str, rec: &Recorder) -> stillwave_plugins::LoadedPlugin {
    PluginHost::new(Limits::default())
        .unwrap()
        .load(&verified(WAT, permissions), Box::new(rec.clone()))
        .unwrap()
}

#[test]
fn signed_plugin_loads_and_calls_one_function() {
    let rec = Recorder::default();
    let mut p = load("", &rec);
    assert_eq!(p.call("hello").unwrap(), 42);
    assert_eq!(*rec.logs.lock().unwrap(), ["org.stillwave.hello: hi"]);
}

#[test]
fn library_read_is_denied_without_permission_and_allowed_with_it() {
    let rec = Recorder::default();
    assert!(matches!(
        load("", &rec).call("count"),
        Err(PluginError::PermissionDenied(Permission::LibraryRead))
    ));
    // `playlists.read` is not `library.read`.
    assert!(matches!(
        load("\"playlists.read\"", &rec).call("count"),
        Err(PluginError::PermissionDenied(Permission::LibraryRead))
    ));
    assert_eq!(load("\"library.read\"", &rec).call("count").unwrap(), 1234);
}

#[test]
fn playlist_write_is_denied_without_permission_and_has_no_side_effect() {
    let rec = Recorder::default();
    assert!(matches!(
        load("\"playlists.read\", \"library.read\"", &rec).call("mkplaylist"),
        Err(PluginError::PermissionDenied(Permission::PlaylistsWrite))
    ));
    assert!(rec.playlists.lock().unwrap().is_empty());
    assert_eq!(
        load("\"playlists.write\"", &rec)
            .call("mkplaylist")
            .unwrap(),
        1
    );
    assert_eq!(*rec.playlists.lock().unwrap(), ["hi"]);
}

#[test]
fn plugin_importing_unknown_host_functions_cannot_load() {
    let wat = r#"(module (import "env" "system" (func (param i32))) (func (export "x") (result i32) i32.const 0))"#;
    let host = PluginHost::new(Limits::default()).unwrap();
    let r = host.load(&verified(wat, ""), Box::new(Recorder::default()));
    assert!(matches!(r, Err(PluginError::Runtime(_))));
}

#[test]
fn runaway_plugin_is_stopped_by_fuel_limit() {
    let host = PluginHost::new(Limits {
        fuel_per_call: 100_000,
        ..Limits::default()
    })
    .unwrap();
    let mut p = host
        .load(&verified(WAT, ""), Box::new(Recorder::default()))
        .unwrap();
    assert!(matches!(p.call("spin"), Err(PluginError::Runtime(_))));
    // The instance stays usable for the next call.
    assert_eq!(p.call("hello").unwrap(), 42);
}

#[test]
fn out_of_bounds_string_pointer_is_an_error_not_a_crash() {
    let rec = Recorder::default();
    assert!(matches!(
        load("", &rec).call("bad_ptr"),
        Err(PluginError::Runtime(_))
    ));
    assert!(rec.logs.lock().unwrap().is_empty());
}

#[test]
fn missing_export_is_a_clean_error() {
    let rec = Recorder::default();
    assert!(matches!(
        load("", &rec).call("nope"),
        Err(PluginError::Runtime(_))
    ));
}

#[test]
fn on_disk_package_round_trip_and_tamper_detection() {
    let dir = tempfile::tempdir().unwrap();
    let wasm = wat::parse_str(WAT).unwrap();
    let manifest = manifest("\"library.read\"");
    let key = generate_signing_key().unwrap();
    std::fs::write(dir.path().join("manifest.toml"), &manifest).unwrap();
    std::fs::write(dir.path().join("plugin.wasm"), &wasm).unwrap();
    std::fs::write(
        dir.path().join("signature.sig"),
        sign(manifest.as_bytes(), &wasm, &key).to_text(),
    )
    .unwrap();
    let trusted = TrustedKeys {
        official: vec![key.verifying_key().to_bytes()],
        community: vec![],
    };
    let app = Version::new(0, 1, 0);

    let pkg = PluginPackage::load_dir(dir.path()).unwrap();
    assert!(verify(&pkg, &trusted, &app).is_ok());

    // Flip one byte of the wasm on disk.
    let mut tampered = wasm.clone();
    *tampered.last_mut().unwrap() ^= 1;
    std::fs::write(dir.path().join("plugin.wasm"), tampered).unwrap();
    let pkg = PluginPackage::load_dir(dir.path()).unwrap();
    assert!(matches!(
        verify(&pkg, &trusted, &app),
        Err(PluginError::BadSignature)
    ));
}

#[test]
fn wrong_key_is_rejected() {
    let wasm = wat::parse_str(WAT).unwrap();
    let manifest = manifest("");
    let (signer, other) = (
        generate_signing_key().unwrap(),
        generate_signing_key().unwrap(),
    );
    let pkg = PluginPackage {
        manifest_bytes: manifest.clone().into_bytes(),
        wasm_bytes: wasm.clone(),
        signature: sign(manifest.as_bytes(), &wasm, &signer),
    };
    let trusted = TrustedKeys {
        official: vec![other.verifying_key().to_bytes()],
        community: vec![],
    };
    assert!(matches!(
        verify(&pkg, &trusted, &Version::new(0, 1, 0)),
        Err(PluginError::UntrustedSigner)
    ));
}
