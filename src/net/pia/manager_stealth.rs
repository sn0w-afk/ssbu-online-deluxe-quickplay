//! Stealth-mode hardening: suppress libssbu_pia_manager's own mod beacon.
//!
//! Reverse-engineering notes (libssbu_pia_manager v1.1.0 / v1.2.0):
//!
//! The manager appends a two-byte extension to its per-station PIA traffic:
//!
//! ```text
//! [0x45, <this console's nn::nifm::GetInternetConnectionStatus type byte>]
//! ```
//!
//! A receiving manager treats the 0x45 tag as proof the station is modded
//! (this is what powers `is_modded`) and clamps the second byte to
//! Wifi(1)/Ethernet(2) — which is what the "[Wired]"/"[Wifi]" ping suffix
//! displays. Vanilla consoles send no tag at all, so ANY suffix, even
//! "[Wifi]", reveals a modded console to anyone running the manager.
//!
//! Stealth mode already keeps us from registering the custom-comms send hook
//! (no 4-byte extended-info packet), but the tag above is emitted by the
//! manager itself, below our layer. To close that leak we patch the tag
//! constant in the manager's `.text` at runtime:
//!
//! ```text
//! mov w8, #0x45   ->   mov w8, #0x0
//! ```
//!
//! The manager still sends its message, but no receiver ever recognizes the
//! tag, so to other players we are indistinguishable from a vanilla console:
//! no is_modded, no interface suffix, no extended info. Our own receive path
//! is untouched — stealth stays antisocial (we still see everyone else).
//!
//! The patch site is located by signature rather than fixed offset, so any
//! manager version sharing the beacon code shape is handled; on an unknown
//! layout we log a warning and leave the manager untouched.

use core::sync::atomic::{AtomicU8, Ordering};

use skyline::patching::patch_pointer;

/// `mov w8, #0x45` (movz) — the beacon tag constant.
const TAG_CONST_MOVZ: u32 = 0x5280_08A8;
/// Replacement: `mov w8, #0x0` — the tag byte is never 0x45 again.
const TAG_CONST_MOVZ_ZERO: u32 = 0x5280_0008;
/// `sturb w8, [xN, #-0x54]` — the store into the beacon buffer. The base
/// register differs across manager versions (x12 in v1.0.0, x19 in v1.1.0+),
/// so bits 9:5 are masked out.
const TAG_STORE_MASK: u32 = 0xFFFF_FC1F;
const TAG_STORE_VALUE: u32 = 0x381A_C008;
/// The store sits within this many instructions after the constant load
/// (v1.1.0/v1.2.0: 2; v1.0.0: 4).
const STORE_LOOKAHEAD: usize = 6;
/// Sanity bound on the manager's text mapping while scanning.
const MAX_SCAN_SIZE: u64 = 0x20_0000;

const STATE_IDLE: u8 = 0; // stealth off — nothing to do
const STATE_ARMED: u8 = 1; // looking for the manager module
const STATE_SETTLED: u8 = 2; // patched, or logged that we couldn't
static PATCH_STATE: AtomicU8 = AtomicU8::new(STATE_IDLE);

/// svcQueryMemory (SVC 0x6) output, padded to the kernel's 0x28-byte struct.
#[repr(C)]
struct MemoryInfo {
    base_address: u64,
    size: u64,
    mem_type: u32,
    attribute: u32,
    perm: u32,
    device_refcount: u32,
    ipc_refcount: u32,
    _padding: u32,
}

const PERM_R: u32 = 1;

/// Returns the mapping containing `addr`, so the signature scan is bounded by
/// the kernel's own memory map instead of guesses about module layout.
unsafe fn query_memory(addr: usize) -> Option<MemoryInfo> {
    let mut info = core::mem::MaybeUninit::<MemoryInfo>::uninit();
    let mut page_info: u32 = 0;
    let result: usize;
    core::arch::asm!(
        "svc #0x6",
        inlateout("x0") info.as_mut_ptr() as usize => result,
        in("x1") &mut page_info,
        in("x2") addr,
        out("x8") _,
        options(nostack),
    );
    (result == 0).then(|| info.assume_init())
}

/// Finds the beacon tag constant load that has its companion buffer store
/// right behind it. (The only other 0x45 immediate in the manager is a
/// `mov w1, #0x45` in a string-formatting helper — different encoding.)
unsafe fn find_beacon_site(text_base: usize, text_size: usize) -> Option<usize> {
    let words = core::slice::from_raw_parts(
        text_base as *const u32,
        text_size / core::mem::size_of::<u32>(),
    );
    for (i, &word) in words.iter().enumerate() {
        if word != TAG_CONST_MOVZ {
            continue;
        }
        let lookahead_end = (i + 1 + STORE_LOOKAHEAD).min(words.len());
        if words[i + 1..lookahead_end]
            .iter()
            .any(|&next| next & TAG_STORE_MASK == TAG_STORE_VALUE)
        {
            return Some(text_base + i * core::mem::size_of::<u32>());
        }
    }
    None
}

/// Arms the patcher when stealth mode is on. Called from pia::install().
pub(super) fn arm() {
    if crate::render::stealth_mode_enabled() {
        PATCH_STATE.store(STATE_ARMED, Ordering::SeqCst);
    }
}

/// Called once per frame from the overlay draw loop. The manager plugin loads
/// after us (alphabetical plugin order), so the symbol lookup usually fails
/// for the first few frames; we retry until the patch lands or proves
/// impossible, then settle and never run again.
pub fn tick() {
    if PATCH_STATE.load(Ordering::SeqCst) != STATE_ARMED {
        return;
    }

    let Some(manager_symbol) = crate::utils::lookup_symbol_addr(b"ssbu_pia_manager_install\0")
    else {
        return; // manager not loaded yet
    };

    let site = unsafe {
        query_memory(manager_symbol).and_then(|info| {
            if info.perm & PERM_R == 0 || info.size == 0 || info.size > MAX_SCAN_SIZE {
                return None;
            }
            find_beacon_site(info.base_address as usize, info.size as usize)
        })
    };

    match site {
        Some(site) => {
            // SAFETY: `site` points at a verified `mov w8, #0x45` inside the
            // manager's text mapping; patch_pointer routes through sky_memcpy,
            // which handles the RX -> RW permission dance.
            let patched = unsafe { patch_pointer(site as *const u8, &TAG_CONST_MOVZ_ZERO) }.is_ok();
            let verified =
                patched && unsafe { *(site as *const u32) } == TAG_CONST_MOVZ_ZERO;
            if verified {
                println!(
                    "[stealth] pia manager beacon suppressed at {site:#x} — modded flag and interface type will not be broadcast"
                );
            } else {
                println!(
                    "[stealth] WARNING: found manager beacon at {site:#x} but patching failed — we may still be identifiable as modded"
                );
            }
        }
        None => println!(
            "[stealth] WARNING: manager beacon signature not found (unknown libssbu_pia_manager version?) — we may still be identifiable as modded"
        ),
    }
    PATCH_STATE.store(STATE_SETTLED, Ordering::SeqCst);
}
