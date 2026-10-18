// SPDX-License-Identifier: GPL-3.0-only
//! Lists playback devices and the formats each accepts in exclusive mode.
//! Makes no sound and does not open a stream. Windows only.
//!
//! `cargo run -p stillwave-audio --example devices`

#[cfg(windows)]
fn main() {
    use stillwave_audio::wasapi::list_devices;
    use stillwave_audio::{OutputDevice, OutputMode};

    let devices = list_devices().expect("could not list devices");
    if devices.is_empty() {
        println!("no active playback devices");
    }
    for d in devices {
        println!(
            "{}{}",
            d.friendly_name,
            if d.is_default { "  [default]" } else { "" }
        );
        match d.supported_formats(OutputMode::Exclusive) {
            Ok(f) if f.is_empty() => println!(
                "    exclusive: no PCM formats accepted (device may not allow exclusive mode)"
            ),
            Ok(f) => {
                let mut by_depth: std::collections::BTreeMap<u8, Vec<u32>> =
                    std::collections::BTreeMap::new();
                for x in &f {
                    by_depth
                        .entry(x.bit_depth)
                        .or_default()
                        .push(x.sample_rate_hz);
                }
                for (depth, rates) in by_depth {
                    println!("    exclusive {depth}-bit: {rates:?} Hz");
                }
            }
            Err(e) => println!("    exclusive: error: {e}"),
        }
        if let Ok(f) = d.supported_formats(OutputMode::Shared) {
            println!("    shared mix format: {}", f[0]);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    println!("this example is Windows-only for now");
}
