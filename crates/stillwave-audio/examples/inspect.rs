// SPDX-License-Identifier: GPL-3.0-only
//! Opens audio files and prints what the decoder sees. Read-only; makes no sound.
//!
//! `cargo run -p stillwave-audio --example inspect -- <file or folder>...`

use std::path::{Path, PathBuf};

use stillwave_audio::{Decoder, FileDecoder};

fn main() {
    let mut files: Vec<PathBuf> = Vec::new();
    for arg in std::env::args().skip(1) {
        collect(Path::new(&arg), &mut files);
    }
    if files.is_empty() {
        eprintln!("usage: inspect <file or folder>...");
        std::process::exit(2);
    }
    for f in files {
        match FileDecoder::open(&f) {
            Ok(mut d) => {
                let mut buf = vec![0i32; 8192];
                let first = d.read(&mut buf);
                println!(
                    "{}: {}{}  frames={:?}  first read={:?}",
                    f.display(),
                    d.format(),
                    if d.is_lossy() { " (lossy)" } else { "" },
                    d.total_frames(),
                    first.map_err(|e| e.to_string())
                );
            }
            Err(e) => println!("{}: ERROR {e}", f.display()),
        }
    }
}

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        if let Ok(rd) = std::fs::read_dir(p) {
            let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            entries.sort();
            for e in entries {
                collect(&e, out);
            }
        }
    } else {
        out.push(p.to_path_buf());
    }
}
