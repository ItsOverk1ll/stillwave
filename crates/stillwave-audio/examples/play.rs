// SPDX-License-Identifier: GPL-3.0-only
//! Plays files through WASAPI and shows the signal path. Windows only.
//!
//! ```text
//! cargo run -p stillwave-audio --example play -- [options] <file>...
//!   --list          list playback devices and exit
//!   --device <n>    use device number n from --list (default: system default)
//!   --shared        use the Windows mixer instead of exclusive mode (not bit-perfect)
//!   --fallback      if exclusive mode is refused, play through the mixer instead
//! ```
//!
//! Exclusive mode takes over the device: other apps go silent while it plays.
//! Turn your volume down first; a lossless file plays at full recorded level.

#[cfg(windows)]
fn main() {
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Duration;

    use stillwave_audio::wasapi::{default_device, list_devices};
    use stillwave_audio::{OutputMode, PlayState, Player, PlayerOptions, QualityBadge};

    let mut files: Vec<PathBuf> = Vec::new();
    let mut device_index: Option<usize> = None;
    let (mut shared, mut fallback, mut list) = (false, false, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--list" => list = true,
            "--shared" => shared = true,
            "--fallback" => fallback = true,
            "--device" => device_index = args.next().and_then(|n| n.parse().ok()),
            _ => files.push(PathBuf::from(a)),
        }
    }

    let devices = list_devices().expect("could not list devices");
    if list {
        for (i, d) in devices.iter().enumerate() {
            println!(
                "{i}: {}{}",
                d.friendly_name,
                if d.is_default { "  [default]" } else { "" }
            );
        }
        return;
    }
    if files.is_empty() {
        eprintln!("usage: play [--list] [--device N] [--shared] [--fallback] <file>...");
        std::process::exit(2);
    }
    let device = match device_index {
        Some(i) => devices.get(i).cloned().expect("no such device number"),
        None => default_device().expect("no default device"),
    };
    println!("device: {}", device.friendly_name);

    let player = Player::new(
        Box::new(device),
        PlayerOptions {
            mode: if shared {
                OutputMode::Shared
            } else {
                OutputMode::Exclusive
            },
            allow_shared_fallback: fallback,
        },
    );
    player.play(files);

    let mut last_index = None;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let st = player.status();
        if st.index != last_index {
            last_index = st.index;
            if let (Some(i), Some(sp)) = (st.index, &st.signal_path) {
                let badge = match sp.badge() {
                    QualityBadge::BitPerfect => "BIT-PERFECT",
                    QualityBadge::Processed => "processed",
                    QualityBadge::Lossy => "lossy source",
                };
                println!(
                    "\n[{i}] {}\n    source {}  ->  output {}  ({badge}, {})",
                    st.queue[i].display(),
                    sp.source,
                    sp.output,
                    if sp.exclusive {
                        "exclusive"
                    } else {
                        "shared mixer"
                    }
                );
            }
        }
        if let (Some(fmt), Some(total)) = (st.format, st.total_frames) {
            let rate = u64::from(fmt.sample_rate_hz);
            print!(
                "\r    {:>3}:{:02} / {:>3}:{:02}",
                st.position_frames / rate / 60,
                st.position_frames / rate % 60,
                total / rate / 60,
                total / rate % 60
            );
            let _ = std::io::stdout().flush();
        }
        if matches!(
            st.state,
            PlayState::Finished | PlayState::Error | PlayState::Stopped
        ) {
            println!();
            if let Some(e) = st.error {
                println!("note: {e}");
            }
            break;
        }
    }
}

#[cfg(not(windows))]
fn main() {
    println!("this example is Windows-only for now");
}
