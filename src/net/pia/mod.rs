use std::sync::{
    atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    Arc, LazyLock, Mutex,
};

pub mod manager_stealth;

use arc_swap::ArcSwap;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use ssbu_pia_interface::{
    self, ConnectedStation, ConnectionChangedEvent, StationConnectionManager,
};

use crate::{
    net::{
        is_in_real_game, is_valid_online_mode,
        latency_slider::{Latency, LatencySliderManager},
    },
    render::profile::{RenderProfile, RenderProfileManager, RenderProfileSettings},
};

const PIA_CUSTOM_COMMS_VERSION: u8 = 3;

#[derive(Debug)]
enum PiaCommsError {
    VersionMismatch,
    InvalidData,
}

static CONNECTED_STATION_TABLE_SYNCED_INTERNAL: LazyLock<Mutex<Vec<StationNetInfo>>> =
    LazyLock::new(|| Mutex::new(Vec::with_capacity(8)));
static CONNECTED_STATION_TABLE_ATOMIC_VIEW: LazyLock<ArcSwap<Vec<StationNetInfo>>> =
    LazyLock::new(|| ArcSwap::from_pointee(Vec::with_capacity(8)));

#[derive(Debug)]
struct StationNetInfo {
    id: u64,
    is_valid_comms: AtomicBool,
    latency_bits: AtomicU8,
    render_profile_settings_bits: AtomicU16,
}

impl Clone for StationNetInfo {
    fn clone(&self) -> Self {
        StationNetInfo {
            id: self.id,
            is_valid_comms: AtomicBool::new(self.is_valid_comms.load(Ordering::SeqCst)),
            latency_bits: AtomicU8::new(self.latency_bits.load(Ordering::SeqCst)),
            render_profile_settings_bits: AtomicU16::new(
                self.render_profile_settings_bits.load(Ordering::SeqCst),
            ),
        }
    }
}

#[repr(C, packed)]
#[derive(Immutable, FromBytes, IntoBytes, KnownLayout, Unaligned, Debug)]
struct PiaCustomNetPacket {
    version: u8,
    latency_bits: u8,
    render_profile_settings_bits: u16,
}

pub trait StationExt {
    fn get_latency(&self) -> Option<Latency>;
    fn get_render_profile_settings(&self) -> Option<RenderProfileSettings>;
    fn get_render_profile(&self) -> Option<RenderProfile>;
}

impl StationExt for ConnectedStation {
    fn get_latency(&self) -> Option<Latency> {
        let id = self.get_id();
        let stations_table = CONNECTED_STATION_TABLE_ATOMIC_VIEW.load();
        stations_table
            .iter()
            .find(|s| s.id == id && s.is_valid_comms.load(Ordering::SeqCst))
            .map(|s| Latency::from_bits(s.latency_bits.load(Ordering::SeqCst)))
    }
    fn get_render_profile_settings(&self) -> Option<RenderProfileSettings> {
        if !self.is_modded() {
            return None;
        }
        let id = self.get_id();
        let stations_table = CONNECTED_STATION_TABLE_ATOMIC_VIEW.load();
        stations_table
            .iter()
            .find(|s| s.id == id && s.is_valid_comms.load(Ordering::SeqCst))
            .and_then(|s| {
                RenderProfileSettings::from_bits(
                    s.render_profile_settings_bits.load(Ordering::SeqCst),
                )
            })
    }
    fn get_render_profile(&self) -> Option<RenderProfile> {
        self.get_render_profile_settings()
            .map(|rps| RenderProfile::from_settings(&rps))
    }
}

fn normalize_and_parse_data(data: &[u8]) -> Result<PiaCustomNetPacket, PiaCommsError> {
    let mut data = match PiaCustomNetPacket::read_from_bytes(data) {
        Ok(data) => data,
        Err(_) => return Err(PiaCommsError::InvalidData),
    };
    if data.version == PIA_CUSTOM_COMMS_VERSION {
        return Ok(data);
    } else if data.version == 2 {
        data.render_profile_settings_bits &= !(1 << 14);
        data.render_profile_settings_bits &= !(1 << 15);
        return Ok(data);
    }
    return Err(PiaCommsError::VersionMismatch);
}

// Lock-free SPSC ring for connection events. The manager fires
// on_station_connection_changed on its own thread — including mid
// session-teardown, where crash report 01787247875 shows the NEX assert
// lands. The callback therefore only reads the station id and queues the
// event; all table/profile work runs later on the render thread, after the
// disturbance grace window closes (see net::process_deferred_net_work).
const CONN_EVENT_RING_SIZE: usize = 16;
const CONN_EVENT_RING_MASK: usize = CONN_EVENT_RING_SIZE - 1;
static CONN_EVENT_ID: [AtomicU64; CONN_EVENT_RING_SIZE] =
    [const { AtomicU64::new(0) }; CONN_EVENT_RING_SIZE];
static CONN_EVENT_META: [AtomicU64; CONN_EVENT_RING_SIZE] =
    [const { AtomicU64::new(0) }; CONN_EVENT_RING_SIZE];
static CONN_EVENT_HEAD: AtomicUsize = AtomicUsize::new(0); // next write (producer)
static CONN_EVENT_TAIL: AtomicUsize = AtomicUsize::new(0); // next read (consumer)

fn push_conn_event(id: u64, event: ConnectionChangedEvent, num_connected: usize) {
    let head = CONN_EVENT_HEAD.load(Ordering::Acquire);
    let tail = CONN_EVENT_TAIL.load(Ordering::Acquire);
    if head.wrapping_sub(tail) >= CONN_EVENT_RING_SIZE {
        // Ring full: drop the oldest event rather than the new one — a lost
        // disconnect would leak a stale station into the table forever.
        CONN_EVENT_TAIL.store(tail.wrapping_add(1), Ordering::Release);
    }
    let slot = head & CONN_EVENT_RING_MASK;
    let meta = (event as u64) | ((num_connected as u64) << 8);
    CONN_EVENT_ID[slot].store(id, Ordering::Relaxed);
    CONN_EVENT_META[slot].store(meta, Ordering::Release);
    CONN_EVENT_HEAD.store(head.wrapping_add(1), Ordering::Release);
}

fn pop_conn_event() -> Option<(u64, ConnectionChangedEvent, usize)> {
    let tail = CONN_EVENT_TAIL.load(Ordering::Acquire);
    let head = CONN_EVENT_HEAD.load(Ordering::Acquire);
    if tail == head {
        return None;
    }
    let slot = tail & CONN_EVENT_RING_MASK;
    let meta = CONN_EVENT_META[slot].load(Ordering::Acquire);
    let id = CONN_EVENT_ID[slot].load(Ordering::Relaxed);
    CONN_EVENT_TAIL.store(tail.wrapping_add(1), Ordering::Release);
    let event = if meta & 0xff == ConnectionChangedEvent::StationConnected as u64 {
        ConnectionChangedEvent::StationConnected
    } else {
        ConnectionChangedEvent::StationDisconnected
    };
    Some((id, event, (meta >> 8) as usize))
}

/// Drains queued connection events on the render thread. Called once per frame
/// from net::process_deferred_net_work, which itself only runs after the
/// transition/disturbance grace window has closed.
pub(super) fn process_pending_connection_events() {
    while let Some((id, event, new_num_connected)) = pop_conn_event() {
        let mut stations_table = CONNECTED_STATION_TABLE_SYNCED_INTERNAL.lock().unwrap();
        if event == ConnectionChangedEvent::StationConnected {
            stations_table.push(StationNetInfo {
                id,
                is_valid_comms: AtomicBool::new(false),
                latency_bits: AtomicU8::new(Latency::unknown().to_bits()),
                render_profile_settings_bits: AtomicU16::new(
                    RenderProfileSettings::vanilla().to_bits(),
                ),
            });
        } else if event == ConnectionChangedEvent::StationDisconnected {
            if let Some(i) = stations_table.iter().position(|s| s.id == id) {
                stations_table.remove(i);
            }
        }

        CONNECTED_STATION_TABLE_ATOMIC_VIEW.store(Arc::new(stations_table.clone()));
        drop(stations_table);

        RenderProfileManager::instance()
            .auto_select_profile(is_valid_online_mode(), new_num_connected > 1);
    }
}

fn on_station_connection_changed(
    event: ConnectionChangedEvent,
    station: ConnectedStation,
    new_num_connected: usize,
) {
    // Manager-thread callback, possibly mid-teardown: read the id, drop the
    // handle, queue the event, restart the grace window. Nothing else.
    let id = station.get_id();
    crate::net::note_connection_disturbance();
    push_conn_event(id, event, new_num_connected);
}

fn send_pia_data_hook(_station: ConnectedStation, data: &mut [u8]) {
    // Only registered when stealth mode is off (see install()).
    let latency_bits = LatencySliderManager::instance()
        .active_latency()
        .unwrap_or_else(|| LatencySliderManager::instance().selected_latency())
        .to_bits();
    let rps_bits = match is_in_real_game() {
        false => RenderProfileManager::instance().selected_render_profile_settings(),
        true => RenderProfileManager::active_render_profile_settings(),
    }
    .to_bits();
    let payload = PiaCustomNetPacket {
        version: PIA_CUSTOM_COMMS_VERSION,
        latency_bits: latency_bits,
        render_profile_settings_bits: rps_bits,
    };
    data.copy_from_slice(payload.as_bytes());
}

fn receive_pia_data_hook(station: ConnectedStation, data: &[u8]) {
    // Version 0 is reserved: stealth-mode peers from v1.2.0-quickplay.2 zeroed
    // their broadcast buffer instead of sending nothing. Treat it exactly like
    // playing against a vanilla console: no extended info is recorded and
    // nothing is logged, so a stealth player leaves no trace in either the
    // overlay or the log.
    if data.first() == Some(&0) {
        return;
    }
    let id = station.get_id();
    let stations_table = CONNECTED_STATION_TABLE_ATOMIC_VIEW.load();
    if let Some(station) = stations_table.iter().find(|s| s.id == id) {
        match normalize_and_parse_data(data) {
            Ok(d) => {
                station.is_valid_comms.store(true, Ordering::SeqCst);
                station.latency_bits.store(d.latency_bits, Ordering::SeqCst);
                station
                    .render_profile_settings_bits
                    .store(d.render_profile_settings_bits, Ordering::SeqCst);
            }
            Err(e) => {
                println!("Error parsing incoming data: {:?}", e);
            }
        }
    }
}

#[cfg(feature = "dummy_connection")]
fn dummy_connection_count() -> usize {
    option_env!("DUMMY_CONNECTIONS")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3)
}

pub(super) fn install() {
    ssbu_pia_interface::install();
    StationConnectionManager::set_enabled(true);
    StationConnectionManager::register_station_connection_changed_callback(
        on_station_connection_changed,
    );
    // Stealth mode: never register the send hook, so we do not participate in
    // the custom-comms broadcast at all — no packet carrying our data is ever
    // handed to the pia manager, for opponents running ANY version of the mod.
    // The receive hook stays registered, so we still see modded opponents'
    // extended info while appearing vanilla to them.
    if !crate::render::stealth_mode_enabled() {
        StationConnectionManager::register_station_data_send_hook(send_pia_data_hook);
    }
    // The manager has its own lower-level beacon (a 0x45 tag + our interface
    // type appended to PIA traffic) that powers is_modded and the
    // "[Wired]"/"[Wifi]" suffix on OTHER consoles. It fires below our layer,
    // so stealth also patches that tag out of the manager's text at runtime.
    manager_stealth::arm();
    StationConnectionManager::register_station_data_received_hook(receive_pia_data_hook);

    #[cfg(feature = "dummy_connection")]
    ssbu_pia_interface::setup_dummy_connection(dummy_connection_count());
}
