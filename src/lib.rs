#![allow(static_mut_refs)]

use std::sync::atomic::{AtomicBool, Ordering};

use skyline::nro::NroInfo;

mod input_poll;
mod net;
mod perf_scaler;
mod render;
mod session_log;
mod ui;
mod utils;

fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().unwrap();

        let msg = match info.payload().downcast_ref::<&'static str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &s[..],
                None => "Box<Any>",
            },
        };

        let err_msg = format!("thread has panicked at '{}', {}\0", msg, location);
        skyline::error::show_error(
            69,
            "Skyline plugin has panicked! Please open the details and send a screenshot to the developer, then close the game.\0",
            err_msg.as_str()
        );
    }));
}

fn on_nro_load(_info: &NroInfo) {
    static NRO_LOADED: AtomicBool = AtomicBool::new(false);
    if NRO_LOADED.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::render::on_nro_load();
}

#[skyline::main(name = "ssbu-online-deluxe")]
pub fn main() {
    setup_panic_hook();

    skyline::nro::add_hook(on_nro_load)
        .expect("Unable to add NRO load hook! Make sure nro hook plugin is present.");

    render::install();
    net::install();
    ui::install();
    perf_scaler::install();

    // Forensic heartbeat — runs last so the BOOT line reflects the final
    // loaded config. Used to correlate crash reports against actual
    // per-session plugin presence.
    session_log::init();
}
