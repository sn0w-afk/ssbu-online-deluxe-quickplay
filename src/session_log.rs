//! Forensic session heartbeat.
//!
//! Crash-report module lists have been observed to omit this plugin (and
//! imgui) from sessions where the mod was verifiably running in-game. Since
//! an nro cannot unload or unregister itself, this log exists to settle per
//! session whether the plugin was alive: a BOOT line at startup, then a TICK
//! line every ~5s with the console RTC epoch (the same clock crash-report
//! filenames use, so the two correlate directly even if the clock is wrong).

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_PATH: &str = "sd:/ultimate/ssbu_online_deluxe/session.log";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn append(line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
    {
        let _ = writeln!(f, "{}", line);
    }
}

pub fn init() {
    append(&format!(
        "BOOT epoch={} version={} stealth={} lurk={} offline_mode={}",
        now_secs(),
        env!("CARGO_PKG_VERSION"),
        crate::render::stealth_mode_enabled(),
        crate::render::lurk_mode_enabled(),
        crate::render::offline_mode_enabled(),
    ));

    // Dedicated thread so the heartbeat does not depend on imgui (the overlay
    // draw loop) or any game hook firing.
    std::thread::Builder::new()
        .stack_size(0x10000)
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            append(&format!(
                "TICK epoch={} valid_online_mode={} connected={} match_status={:?} transition_active={}",
                now_secs(),
                crate::net::is_valid_online_mode(),
                crate::net::is_connected(),
                crate::net::get_match_status(),
                crate::net::is_scene_transition_active(),
            ));
        })
        .expect("Unable to spawn session log thread!");
}
