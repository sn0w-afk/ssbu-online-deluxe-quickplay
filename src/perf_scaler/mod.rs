use std::sync::atomic::{AtomicU8, Ordering};
use ultelier::sync_guest::{self, ResolutionLevel};

mod common;
mod fighter;
mod utils;

use crate::render::profile::RenderProfileManager;

static ENTRY_BASE_RESOLUTION: AtomicU8 = AtomicU8::new(ResolutionLevel::Res1920x1080 as u8);

pub(in crate::perf_scaler) fn push_dynamic_res_report() {
    let base_res_level =
        ResolutionLevel::from_u32(ENTRY_BASE_RESOLUTION.load(Ordering::SeqCst) as u32).unwrap();
    let target_res = if base_res_level >= ResolutionLevel::Res1024x576 {
        ResolutionLevel::Res854x480
    } else if base_res_level >= ResolutionLevel::Res1280x720 {
        ResolutionLevel::Res1024x576
    } else {
        ResolutionLevel::Res1280x720
    };

    sync_guest::push_dynamic_res_report(target_res);
}

pub(in crate::perf_scaler) fn pop_dynamic_res_report() {
    let base_res_level =
        ResolutionLevel::from_u32(ENTRY_BASE_RESOLUTION.load(Ordering::SeqCst) as u32).unwrap();
    let target_res = if base_res_level >= ResolutionLevel::Res1024x576 {
        ResolutionLevel::Res854x480
    } else if base_res_level >= ResolutionLevel::Res1280x720 {
        ResolutionLevel::Res1024x576
    } else {
        ResolutionLevel::Res1280x720
    };

    sync_guest::pop_dynamic_res_report(target_res);
}

static PENDING_DRS_POP: AtomicU8 = AtomicU8::new(0);

/// Gated variant of `pop_dynamic_res_report` for the critical-hit DRS path.
/// A finishing move's slow-mo can end right as the match -> results-stage
/// transition begins, so popping during the grace window would call into
/// ssbusync mid-load. The pop is deferred and drained once the window closes;
/// push/pop stay balanced either way.
pub(in crate::perf_scaler) fn pop_dynamic_res_report_gated() {
    if crate::net::is_scene_transition_active() {
        PENDING_DRS_POP.fetch_add(1, Ordering::SeqCst);
    } else {
        pop_dynamic_res_report();
    }
}

/// Drains deferred DRS pops. Called once per frame from the overlay draw loop.
pub(crate) fn process_deferred_drs_pop() {
    if crate::net::is_scene_transition_active() {
        return;
    }
    while PENDING_DRS_POP.load(Ordering::SeqCst) > 0 {
        PENDING_DRS_POP.fetch_sub(1, Ordering::SeqCst);
        pop_dynamic_res_report();
    }
}

pub(crate) fn match_init() {
    sync_guest::clear_all_dynamic_res_report();
    let rps = RenderProfileManager::active_render_profile_settings();
    let base_res_level = rps.default_resolution_level();
    ENTRY_BASE_RESOLUTION.store(base_res_level as u8, Ordering::SeqCst);
    common::init();
    fighter::init();
}

pub(crate) fn match_cleanup() {
    // clear_all resets ssbusync's report stack, so any deferred pops are moot
    // and must be dropped to keep push/pop balanced.
    PENDING_DRS_POP.store(0, Ordering::SeqCst);
    sync_guest::clear_all_dynamic_res_report();
}

pub(super) fn install() {
    common::install();
    fighter::install();
}
