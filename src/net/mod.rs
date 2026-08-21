pub mod latency_slider;
pub mod ldn;
pub mod pia;

use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};

use crate::net::ldn::interface::{get_network_role, NetworkRole};
use skyline::hooks::InlineCtx;
use skyline::nn::ui2d::Pane;
use ssbu_pia_interface::StationConnectionManager;

static mut LOCAL_ONLINE_CSS_NUM_PANES_ADJUSTED: bool = false;
static mut CURRENT_ARENA_ID: String = String::new();

static ONLINE_ARENA_PANE_HANDLE: AtomicU64 = AtomicU64::new(0);
static LOCAL_ROOM_PANE_HANDLE: AtomicU64 = AtomicU64::new(0);

static MATCH_CONNECTION_STATUS: AtomicU8 = AtomicU8::new(MatchConnectionStatus::Offline as u8);
static MATCH_STATUS: AtomicU8 = AtomicU8::new(MatchStatus::Inactive as u8);

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchConnectionStatus {
    Offline = 0,
    OnlineLocal = 1,
    OnlineArena = 2,
    OnlineQuickplay = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    Inactive = 0,
    Singles = 1,
    Doubles = 2,
    Training = 3,
}

extern "C" {
    #[link_name = "\u{1}_ZN3app9smashball16is_training_modeEv"]
    fn is_training_mode() -> bool;

    #[link_name = "\u{1}_ZN3app7fighter23get_fighter_entry_countEv"]
    fn get_fighter_entry_count() -> i32;
}

/// Self-tracked scene transition detection.
///
/// Crash-report analysis (10 reports) showed the dominant failure is an nnSdk
/// assertion on a networking-adjacent system thread that fires during scene
/// transitions (quickplay -> CSS on no-rematch, arena CSS reloads). Anything
/// that pokes network/render state during that window is a suspect, so
/// per-frame actors must be gated while a transition is in progress.
///
/// The game exposes no reliable public "is loading" symbol (the first attempt
/// linked app::smashball::is_loading, which does not resolve in SSBU 13.0.x
/// and crashed on startup), so instead every scene-init / stage-load hook we
/// already own starts a frame-count grace period, and the overlay draw loop
/// ticks it down. 300 frames (~5s at 60fps) comfortably covers SSBU's loads.
const TRANSITION_GRACE_FRAME_COUNT: u64 = 300;
static TRANSITION_GRACE_FRAMES: AtomicU64 = AtomicU64::new(TRANSITION_GRACE_FRAME_COUNT);

/// Called from scene-init and stage-load hooks: a transition is starting.
pub fn note_scene_transition() {
    TRANSITION_GRACE_FRAMES.store(TRANSITION_GRACE_FRAME_COUNT, Ordering::SeqCst);
}

/// Called once per frame from the overlay draw loop.
pub fn tick_scene_transition() {
    let frames = TRANSITION_GRACE_FRAMES.load(Ordering::SeqCst);
    if frames > 0 {
        TRANSITION_GRACE_FRAMES.store(frames - 1, Ordering::SeqCst);
    }
}

#[inline]
pub fn is_scene_transition_active() -> bool {
    TRANSITION_GRACE_FRAMES.load(Ordering::SeqCst) > 0
}

/// Connection disturbances ride the same grace window as scene transitions.
///
/// Crash report 01787247875 showed why: in quickplay no-rematch, the NEX
/// session teardown trails the CSS load by ~10 seconds — the assert fired 3s
/// after the pia disconnect and 13s after the transition grace had expired.
/// Gating only on scene transitions leaves the teardown window uncovered, so
/// every station connect/disconnect restarts the grace period as well.
pub fn note_connection_disturbance() {
    note_scene_transition();
}

const PENDING_MATCH_NONE: u8 = 0;
const PENDING_MATCH_INIT: u8 = 1;
const PENDING_MATCH_CLEANUP: u8 = 2;
static PENDING_MATCH_APPLY: AtomicU8 = AtomicU8::new(PENDING_MATCH_NONE);
static PENDING_ARENA_MARK: AtomicU8 = AtomicU8::new(0);

/// Defers the render/perf profile swap out of scene hooks.
///
/// `match_init`/`match_cleanup` write render env flags via sync_guest and poke
/// the OC sysmodule — exactly the kind of work that must not run while a scene
/// load or session teardown is in flight. Scene hooks now only record which
/// apply is pending; the swap executes from `process_deferred_net_work` once
/// the transition/disturbance grace has fully expired. The arena mark is
/// drained first in that same pass, so the profile always applies into an
/// already-unrestricted session.
fn request_match_apply(pending: u8) {
    PENDING_MATCH_APPLY.store(pending, Ordering::SeqCst);
}

/// Marks the session for ssbusync after the grace window instead of inside a
/// scene-init hook. ssbusync clears its mode flags on every transition anyway,
/// so marking mid-transition is both wasted and risky.
fn request_arena_mark() {
    PENDING_ARENA_MARK.store(1, Ordering::SeqCst);
}

/// Runs once per frame from the overlay draw loop. Executes every piece of
/// deferred network/render work — but only after the transition/disturbance
/// grace window has fully closed, so nothing we do can interleave with scene
/// loads or NEX session teardown.
pub fn process_deferred_net_work() {
    if is_scene_transition_active() {
        return;
    }
    if PENDING_ARENA_MARK.swap(0, Ordering::SeqCst) != 0 {
        mark_arena_mode_for_ssbusync();
    }
    pia::process_pending_connection_events();
    match PENDING_MATCH_APPLY.swap(PENDING_MATCH_NONE, Ordering::SeqCst) {
        PENDING_MATCH_INIT => {
            crate::render::profile::match_init();
            crate::perf_scaler::match_init();
        }
        PENDING_MATCH_CLEANUP => {
            crate::perf_scaler::match_cleanup();
            crate::render::profile::match_cleanup();
        }
        _ => {}
    }
}

#[skyline::hook(offset = 0x235a650, inline)]
unsafe fn main_menu_init(_: &InlineCtx) {
    note_scene_transition();
    LOCAL_ROOM_PANE_HANDLE.store(0, Ordering::SeqCst);
    ONLINE_ARENA_PANE_HANDLE.store(0, Ordering::SeqCst);
    MATCH_CONNECTION_STATUS.store(MatchConnectionStatus::Offline as u8, Ordering::SeqCst);
    update_match_status(MatchStatus::Inactive, false);
}

#[skyline::hook(offset = 0x22d9d10, inline)]
unsafe fn online_melee_any_init(_: &InlineCtx) {
    note_scene_transition();
    LOCAL_ROOM_PANE_HANDLE.store(0, Ordering::SeqCst);
    ONLINE_ARENA_PANE_HANDLE.store(0, Ordering::SeqCst);
    MATCH_CONNECTION_STATUS.store(MatchConnectionStatus::OnlineQuickplay as u8, Ordering::SeqCst);
    update_match_status(MatchStatus::Inactive, false);
    request_arena_mark();
}

#[skyline::hook(offset = 0x22d9c40, inline)]
unsafe fn online_bg_matchmaking_init(_: &InlineCtx) {
    note_scene_transition();
    LOCAL_ROOM_PANE_HANDLE.store(0, Ordering::SeqCst);
    ONLINE_ARENA_PANE_HANDLE.store(0, Ordering::SeqCst);
    MATCH_CONNECTION_STATUS.store(MatchConnectionStatus::OnlineQuickplay as u8, Ordering::SeqCst);
    update_match_status(MatchStatus::Inactive, false);
    request_arena_mark();
}

#[skyline::hook(offset = 0x22d9b50, inline)]
unsafe fn online_arena_menu_init(_: &InlineCtx) {
    note_scene_transition();
    MATCH_CONNECTION_STATUS.store(MatchConnectionStatus::OnlineArena as u8, Ordering::SeqCst);
}

#[skyline::hook(offset = 0x18881f0, inline)]
unsafe fn online_arena_room_update(_: &skyline::hooks::InlineCtx) {
    let pane_handle = ONLINE_ARENA_PANE_HANDLE.load(Ordering::SeqCst) as *mut u64 as *mut Pane;
    if !pane_handle.is_null() {
        crate::ui::native::update_online_arena_ui(pane_handle, CURRENT_ARENA_ID.clone());
    }
}

#[skyline::hook(offset = 0x1887b1c, inline)]
unsafe fn online_arena_room_init(ctx: &skyline::hooks::InlineCtx) {
    note_scene_transition();
    let panel = *((*((ctx.registers[0].x() + 8) as *const u64) + 0x10) as *const u64);
    ONLINE_ARENA_PANE_HANDLE.store(panel, Ordering::SeqCst);
    CURRENT_ARENA_ID = String::from_utf16(std::slice::from_raw_parts(
        ctx.registers[3].x() as *const u16,
        5,
    ))
    .unwrap();
    update_match_status(MatchStatus::Inactive, false);
}

// called on local online menu init
#[skyline::hook(offset = 0x1bd45e0, inline)]
unsafe fn online_local_menu_init(ctx: &InlineCtx) {
    note_scene_transition();
    println!("LOCAL ONLINE INIT");
    LOCAL_ONLINE_CSS_NUM_PANES_ADJUSTED = false;
    let handle = *((*((ctx.registers[0].x() + 8) as *const u64) + 0x10) as *const u64);
    LOCAL_ROOM_PANE_HANDLE.store(handle, Ordering::SeqCst);
    MATCH_CONNECTION_STATUS.store(MatchConnectionStatus::OnlineLocal as u8, Ordering::SeqCst);
    update_match_status(MatchStatus::Inactive, false);
}

#[skyline::hook(offset = 0x1bd7a80, inline)]
unsafe fn online_local_menu_update(_: &InlineCtx) {
    let pane_handle = LOCAL_ROOM_PANE_HANDLE.load(Ordering::SeqCst) as *mut u64 as *mut Pane;
    if !pane_handle.is_null() {
        crate::ui::native::update_local_online_ui(pane_handle);
    }
}

#[skyline::hook(offset = 0x1a26200)]
unsafe fn css_player_pane_num_changed(param_1: i64, prev_num: i32, changed_by_player: u32) {
    if is_local_online_mode()
        && !LOCAL_ONLINE_CSS_NUM_PANES_ADJUSTED
        && changed_by_player == 0
        && get_network_role() == NetworkRole::Host
    {
        LOCAL_ONLINE_CSS_NUM_PANES_ADJUSTED = true;
        *((param_1 + 0x160) as *mut i32) = 2;
    }
    update_match_status(MatchStatus::Inactive, false);
    call_original!(param_1, prev_num, changed_by_player);
}

#[skyline::hook(offset = 0x1a12f60)]
unsafe fn update_css(arg: u64) {
    // The banner pane chain is walked without the game guaranteeing the nodes
    // exist yet — during CSS loads (arena character switches, quickplay
    // no-rematch returns) the intermediate pointers can be null or the layout
    // half-built. Skip entirely while a transition is active, and null-check
    // every link regardless: a missed UI frame is cheap, a Data Abort is not.
    if is_valid_online_mode() && !is_scene_transition_active() {
        // Original chain: pane = *( *((arg+0xe58) as u64*) + 0x10 ) as Pane*
        let l1 = *((arg + 0xe58) as *const u64);
        let l2 = if l1 == 0 {
            0
        } else {
            *((l1 + 0x10) as *const u64)
        };
        let banner_pane1_ptr = l2 as *mut Pane;
        if !banner_pane1_ptr.is_null() {
            crate::ui::native::update_css_ui(banner_pane1_ptr);
        }
    }
    call_original!(arg);
}

fn update_match_status(match_status: MatchStatus, force_update: bool) {
    // Fallback: if a real pia-connected match is starting while no online mode
    // is being tracked, it can only be a quickplay match (arena and local
    // online are tracked by their own scene hooks). This happens when the
    // player passes through the main menu during background matchmaking,
    // which resets the tracked connection status to Offline.
    if matches!(match_status, MatchStatus::Singles | MatchStatus::Doubles)
        && MATCH_CONNECTION_STATUS.load(Ordering::SeqCst) == MatchConnectionStatus::Offline as u8
        && is_connected()
    {
        println!("UNTRACKED ONLINE MATCH DETECTED, ASSUMING QUICKPLAY");
        MATCH_CONNECTION_STATUS.store(MatchConnectionStatus::OnlineQuickplay as u8, Ordering::SeqCst);
    }

    let prev = MATCH_STATUS.swap(match_status as u8, Ordering::SeqCst);
    if prev != match_status as u8 || force_update {
        println!("UPDATE MATCH STATUS: {:?}", match_status);
        if match_status != MatchStatus::Inactive {
            // Deferred: env-flag writes and OC sysmodule pokes must not run
            // inside this scene hook (fires mid-load via on_stage_presetup).
            request_match_apply(PENDING_MATCH_INIT);
        } else {
            latency_slider::match_cleanup();
            request_match_apply(PENDING_MATCH_CLEANUP);
        }
    }
}

#[inline]
pub fn is_local_online_mode() -> bool {
    MATCH_CONNECTION_STATUS.load(Ordering::SeqCst) == MatchConnectionStatus::OnlineLocal as u8
}

#[inline]
pub fn is_online_arena_mode() -> bool {
    MATCH_CONNECTION_STATUS.load(Ordering::SeqCst) == MatchConnectionStatus::OnlineArena as u8
}

#[inline]
pub fn is_online_quickplay_mode() -> bool {
    MATCH_CONNECTION_STATUS.load(Ordering::SeqCst) == MatchConnectionStatus::OnlineQuickplay as u8
}

#[inline]
pub fn is_valid_online_mode() -> bool {
    #[cfg(feature = "dummy_connection")]
    return true;

    #[cfg(not(feature = "dummy_connection"))]
    return is_online_arena_mode() || is_local_online_mode() || is_online_quickplay_mode();
}

#[inline]
pub fn is_connected() -> bool {
    return StationConnectionManager::is_connected();
}

#[inline]
pub fn is_in_game() -> bool {
    MATCH_STATUS.load(Ordering::SeqCst) != MatchStatus::Inactive as u8
}

#[inline]
pub fn is_in_real_game() -> bool {
    let match_status = MATCH_STATUS.load(Ordering::SeqCst);
    match_status == MatchStatus::Singles as u8 || match_status == MatchStatus::Doubles as u8
}

#[inline]
pub fn get_match_status() -> MatchStatus {
    match MATCH_STATUS.load(Ordering::SeqCst) {
        1 => MatchStatus::Singles,
        2 => MatchStatus::Doubles,
        3 => MatchStatus::Training,
        _ => MatchStatus::Inactive,
    }
}

const SSBUSYNC_MARK_ARENA_MODE_SYMBOL: &[u8] = b"ssbusync_restrict_mark_arena_mode\0";
static MARK_ARENA_MODE_ADDR: AtomicUsize = AtomicUsize::new(0);
static MARK_ARENA_MODE_COUNTDOWN: AtomicU64 = AtomicU64::new(0);

fn mark_arena_mode_addr() -> Option<usize> {
    match MARK_ARENA_MODE_ADDR.load(Ordering::Acquire) {
        0 => {
            let addr = crate::utils::lookup_symbol_addr(SSBUSYNC_MARK_ARENA_MODE_SYMBOL)
                .unwrap_or(usize::MAX);
            MARK_ARENA_MODE_ADDR.store(addr, Ordering::Release);
            if addr == usize::MAX {
                None
            } else {
                Some(addr)
            }
        }
        usize::MAX => None,
        addr => Some(addr),
    }
}

/// ssbusync restricts its runtime optimizations (vsync off, double buffering,
/// etc.) to offline, arena, and local-online play. When a pia connection is
/// established without a recognized online mode marked, it forces the vanilla
/// runtime and keeps rejecting any non-vanilla env-flag request while in that
/// state — which is exactly the quickplay/Elite Smash case. ssbusync exports
/// `ssbusync_restrict_mark_arena_mode` so companion plugins can mark the
/// session as an arena; calling it during quickplay keeps the selected render
/// profile from being force-reverted to Vanilla.
pub fn mark_arena_mode_for_ssbusync() {
    if let Some(addr) = mark_arena_mode_addr() {
        let func: extern "C" fn() = unsafe { std::mem::transmute(addr) };
        func();
    }
}

/// Throttled variant of `mark_arena_mode_for_ssbusync` for per-frame callers.
/// ssbusync clears its online-mode flags on every menu/scene transition, so
/// the arena mark must be refreshed periodically; but each call also writes a
/// log line, so this marks at most once every ~60 frames to avoid log spam.
pub fn mark_arena_mode_for_ssbusync_throttled() {
    let countdown = MARK_ARENA_MODE_COUNTDOWN.load(Ordering::Relaxed);
    if countdown > 0 {
        MARK_ARENA_MODE_COUNTDOWN.store(countdown - 1, Ordering::Relaxed);
        return;
    }
    MARK_ARENA_MODE_COUNTDOWN.store(60, Ordering::Relaxed);
    mark_arena_mode_for_ssbusync();
}

/// Keeps ssbusync's arena mark set whenever a quickplay restriction could
/// apply: while the quickplay scene is being tracked, or while a pia
/// connection exists (covers background matchmaking, where the tracked
/// connection status may have been reset by passing through the main menu).
///
/// Skipped during scene transitions: ssbusync clears its mode flags on every
/// transition anyway, and calling into it while the session/render state is
/// mid-teardown is the prime suspect for the nnSdk assertion crashes seen on
/// loading screens (see crash report analysis).
pub fn enforce_ssbusync_arena_mark() {
    if is_scene_transition_active() {
        return;
    }
    if is_online_quickplay_mode() || is_connected() {
        mark_arena_mode_for_ssbusync_throttled();
    }
}

#[inline]
pub fn is_in_valid_online_game() -> bool {
    is_valid_online_mode() && is_in_real_game() && is_connected()
}

#[skyline::hook(offset = 0x25d8e38, inline)]
unsafe fn on_stage_presetup(ctx: &InlineCtx) {
    note_scene_transition();
    let stage_base = ctx.registers[0].x();
    let stage_id = *((stage_base + 8) as *mut u32);

    let is_training_mode = is_training_mode();
    let is_waiting_room_stage = stage_id == 311;

    println!(
        "STAGE PRESETUP: STAGE_ID={}, IS_TRAINING_MODE={}",
        stage_id, is_training_mode
    );

    // result stage (normal) == 310
    // result stage (sephiroth) == 354
    let is_result_stage = stage_id == 310 || stage_id == 354;
    if is_result_stage {
        update_match_status(MatchStatus::Inactive, false);
        return;
    }

    if is_training_mode || is_waiting_room_stage {
        update_match_status(MatchStatus::Training, true);
        return;
    }

    let fighter_entry_count = get_fighter_entry_count();
    let match_status = if fighter_entry_count > 2 {
        MatchStatus::Doubles
    } else {
        MatchStatus::Singles
    };
    update_match_status(match_status, true);
}

pub(super) fn install() {
    skyline::install_hooks!(
        main_menu_init,
        online_melee_any_init,
        online_bg_matchmaking_init,
        online_arena_menu_init,
        online_arena_room_init,
        online_arena_room_update,
        online_local_menu_init,
        online_local_menu_update,
        update_css,
        css_player_pane_num_changed,
        on_stage_presetup,
    );
    latency_slider::install();
    ldn::install();
    pia::install();
}
