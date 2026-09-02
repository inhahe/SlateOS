//! `hwsim` — simulated 802.11 radios and the medium between them.
//!
//! This is the wireless counterpart of [`veth`](super::veth): a set of virtual
//! radios with no hardware anywhere, plus a shared "air" that carries frames
//! between them. It exists because the rest of the WiFi stack is written and
//! has nothing to run on — `net80211` (frames, information elements, EAPOL,
//! clause-12 key derivation) and `net80211::supplicant` (scan → auth → assoc →
//! 4-way handshake) are complete and unit-tested, but QEMU emulates no wireless
//! device, so not one of those frames has ever crossed anything.
//!
//! Linux has exactly this and calls it `mac80211_hwsim`; almost all of Linux's
//! own WiFi testing runs against it, and it stays useful after real drivers
//! exist because it is the only way to test both ends of a link on one machine.
//!
//! ## The model
//!
//! - A **radio** is a MAC address, a channel, an up/down flag, a bounded RX
//!   queue, and a key slot. Radios are created and destroyed at runtime.
//! - The **medium** is implicit: a frame transmitted by radio *X* is offered to
//!   every *other* radio that is up and tuned to the same channel. Channel is
//!   the only isolation — same as `mac80211_hwsim`'s default (no `wmediumd`),
//!   where every radio hears every other on its channel.
//! - Each receiving radio then applies its **address filter**, exactly as real
//!   hardware does: a frame is accepted if Address 1 (the receiver address) is
//!   this radio's MAC or a group address. A radio in promiscuous mode (monitor)
//!   accepts everything. Frames rejected by the filter are counted, not
//!   silently discarded, so a test can prove that a frame addressed elsewhere
//!   did *not* arrive.
//!
//! ## What this proves, and what it does not
//!
//! **It does not encrypt.** CCMP is done by the radio on real hardware, so
//! `install_pairwise_key` and `install_group_key` record key material rather
//! than applying it, and a frame with the Protected bit set crosses this medium
//! in the clear. What a simulated association therefore proves is the *frame
//! exchange* and the *key schedule* — that both ends derive the same PTK and
//! that the handshake reaches `Complete` — not confidentiality. Anything
//! claiming otherwise from a green hwsim run would be claiming something the
//! medium never checked.
//!
//! **It does check key reinstallation**, which is the one security property a
//! simulated radio can check better than real hardware can. Installing a key
//! resets the packet number that CCMP uses as a nonce, so installing the *same*
//! key twice replays a nonce and leaks keystream — this is KRACK (Vanhoef &
//! Piessens, CCS 2017). Real hardware does as it is told; this radio refuses a
//! reinstall of identical key material with [`KernelError::AlreadyExists`],
//! counts the refusal, and — importantly — leaves the packet number where it
//! was. A driver that mishandles a retransmitted message 3 fails here instead
//! of shipping.
//!
//! ## Integration
//!
//! - `net80211::frame::MacHeader` parses every frame offered to
//!   [`transmit`]. A buffer that is not a well-formed 802.11 header is
//!   rejected rather than carried, so everything on this medium is a frame.
//! - Nothing is wired into `net::poll()` yet: there is no path from an 802.11
//!   data frame to the IP stack until a consumer strips LLC/SNAP
//!   (`net80211::llc`). That consumer is lane C's and is the next step.
//!
//! ## References
//!
//! - Linux `drivers/net/wireless/virtual/mac80211_hwsim.c`
//! - IEEE Std 802.11-2020, clause 9 (frame formats), clause 12 (security)
//! - `requests/c-a-the-wifi-handshake-is-written-and-has-nothing-to-run-on.md`

// Subsystem API surface; the consumer that drives it is lane C's and lands next.
#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use net80211::frame::MacHeader;
use net80211::{MacAddr, is_group_addr};

use crate::error::{KernelError, KernelResult};
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of simulated radios.
///
/// Eight is enough for the tests this exists to make possible — an AP and a
/// handful of stations, or two independent BSSs on different channels — and
/// keeps the table a fixed, pre-sized allocation.
pub const MAX_RADIOS: usize = 8;

/// Frames buffered per radio before the oldest are dropped.
///
/// A join is a handful of frames, so this is sized for a burst of data traffic
/// rather than for the handshake: 64 × 2352 bytes is ~147 KiB worst case.
pub const RX_QUEUE_DEPTH: usize = 64;

/// Largest frame the medium will carry, in octets.
///
/// The 802.11-2020 maximum MPDU for a non-aggregated frame: 2304 octets of
/// payload plus the largest MAC header, CCMP header, MIC and FCS.
pub const MAX_FRAME_LEN: usize = 2352;

/// The channel a newly created radio is tuned to.
///
/// Channel 6 is the middle of the three non-overlapping 2.4 GHz channels and is
/// what most defaults pick; the value matters only in that two radios must
/// agree, and picking one keeps a two-radio test from having to set it.
pub const DEFAULT_CHANNEL: u8 = 6;

/// The 5 GHz channel numbers this medium accepts.
///
/// A list rather than a range because 5 GHz channel numbering is not contiguous
/// — the 20 MHz channels step by 4, with gaps between bands. Refusing a channel
/// that does not exist keeps a typo from silently creating a private medium that
/// nothing else can ever join, which is a hard failure to read from the far end
/// ("my frames go nowhere") .
const CHANNELS_5GHZ: [u8; 25] = [
    36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144,
    149, 153, 157, 161, 165,
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Identifier for a simulated radio (index into the global table).
pub type RadioId = usize;

/// Which key slot an installed key occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// The pairwise transient key — protects unicast traffic with one peer.
    Pairwise,
    /// A group temporal key — protects broadcast/multicast from the AP.
    Group,
}

/// Key material recorded by a radio.
///
/// The bytes are kept so that a *reinstall* of identical material can be
/// recognised and refused; nothing here encrypts anything.
#[derive(Debug, Clone)]
struct InstalledKey {
    kind: KeyKind,
    /// Key index (0–3). Always 0 for a pairwise key.
    key_id: u8,
    key: Vec<u8>,
    /// How many times a *distinct* key has been installed in this slot. A
    /// refused reinstall does not advance it.
    installs: u64,
}

/// One simulated radio.
struct Radio {
    /// Whether this slot is in use.
    active: bool,
    /// This radio's MAC address (locally administered).
    mac: MacAddr,
    /// The channel this radio is tuned to.
    channel: u8,
    /// Whether the radio is administratively up. A radio that is down neither
    /// transmits nor receives.
    up: bool,
    /// Monitor mode: accept every frame on the channel, not only those whose
    /// Address 1 matches.
    promiscuous: bool,
    /// Frames received from the medium, oldest first.
    rx_queue: VecDeque<Vec<u8>>,
    /// The pairwise key slot.
    pairwise: Option<InstalledKey>,
    /// The group key slot.
    group: Option<InstalledKey>,
    /// The CCMP packet number for the pairwise key.
    ///
    /// Advanced once per transmitted frame that has the Protected bit set, and
    /// reset to zero when a *new* pairwise key is installed. A refused
    /// reinstall deliberately leaves it alone: that is the whole property this
    /// counter exists to make observable. The group key's own PN space is not
    /// modelled, because nothing yet transmits a protected group-addressed
    /// frame.
    pairwise_pn: u64,
    /// Reinstall attempts refused (see the module docs on KRACK).
    key_reinstalls_refused: u64,
    tx_frames: u64,
    tx_bytes: u64,
    rx_frames: u64,
    rx_bytes: u64,
    /// Frames the medium offered but the RX queue had no room for.
    rx_dropped_full: u64,
    /// Frames the medium offered but the address filter rejected.
    rx_filtered: u64,
}

impl Radio {
    fn empty() -> Self {
        Self {
            active: false,
            mac: [0; 6],
            channel: DEFAULT_CHANNEL,
            up: false,
            promiscuous: false,
            rx_queue: VecDeque::new(),
            pairwise: None,
            group: None,
            pairwise_pn: 0,
            key_reinstalls_refused: 0,
            tx_frames: 0,
            tx_bytes: 0,
            rx_frames: 0,
            rx_bytes: 0,
            rx_dropped_full: 0,
            rx_filtered: 0,
        }
    }

    /// True if this radio's address filter accepts a frame addressed to `addr1`.
    fn accepts(&self, addr1: &MacAddr) -> bool {
        self.promiscuous || *addr1 == self.mac || is_group_addr(addr1)
    }

    /// Offer a frame to this radio, applying the address filter and the queue
    /// bound. Returns true if it was queued.
    fn offer(&mut self, addr1: &MacAddr, frame: &[u8]) -> bool {
        if !self.accepts(addr1) {
            self.rx_filtered = self.rx_filtered.saturating_add(1);
            return false;
        }
        if self.rx_queue.len() >= RX_QUEUE_DEPTH {
            // Tail drop, not head drop: a full queue means the consumer is not
            // draining, and the frames it has not looked at yet are the older
            // ones. Discarding those to make room would silently reorder a
            // handshake, which is the single worst failure this module could
            // hand a state machine.
            self.rx_dropped_full = self.rx_dropped_full.saturating_add(1);
            return false;
        }
        self.rx_queue.push_back(frame.to_vec());
        self.rx_frames = self.rx_frames.saturating_add(1);
        self.rx_bytes = self.rx_bytes.saturating_add(frame.len() as u64);
        true
    }
}

/// Per-radio counters and configuration, for `list`/`stats`/`/proc`.
#[derive(Debug, Clone)]
pub struct RadioStats {
    /// Radio ID.
    pub id: RadioId,
    /// MAC address.
    pub mac: MacAddr,
    /// Channel the radio is tuned to.
    pub channel: u8,
    /// Whether the radio is up.
    pub up: bool,
    /// Whether the address filter is disabled (monitor mode).
    pub promiscuous: bool,
    /// Whether a pairwise key is installed.
    pub pairwise_installed: bool,
    /// Whether a group key is installed.
    pub group_installed: bool,
    /// Distinct pairwise keys installed so far.
    pub pairwise_installs: u64,
    /// The CCMP packet number for the pairwise key.
    pub pairwise_pn: u64,
    /// Reinstall attempts refused.
    pub key_reinstalls_refused: u64,
    /// Frames transmitted.
    pub tx_frames: u64,
    /// Bytes transmitted.
    pub tx_bytes: u64,
    /// Frames received (queued).
    pub rx_frames: u64,
    /// Bytes received (queued).
    pub rx_bytes: u64,
    /// Frames dropped because the RX queue was full.
    pub rx_dropped_full: u64,
    /// Frames rejected by the address filter.
    pub rx_filtered: u64,
    /// Frames currently waiting to be read.
    pub rx_pending: usize,
}

struct RadioTable {
    radios: Vec<Radio>,
    /// Monotonic counter feeding MAC generation, so a destroyed-and-recreated
    /// radio does not reuse the address a peer may still have cached.
    mac_counter: u32,
}

impl RadioTable {
    fn new() -> Self {
        let mut radios = Vec::with_capacity(MAX_RADIOS);
        for _ in 0..MAX_RADIOS {
            radios.push(Radio::empty());
        }
        Self {
            radios,
            mac_counter: 0,
        }
    }
}

static TABLE: Mutex<Option<RadioTable>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the simulated-radio subsystem.
///
/// Idempotent, and not a precondition for anything below — the accessors build
/// the table on first use. Calling it forces that to happen at a known point in
/// boot (after the heap, since the table needs `Vec`) and logs it.
pub fn init() {
    let mut table = TABLE.lock();
    let _ = table.get_or_insert_with(RadioTable::new);
    crate::serial_println!("[hwsim] Initialized ({} max radios)", MAX_RADIOS);
}

// The `Option` is an initialization-order artifact — `Mutex::new` needs a const
// initializer and `RadioTable::new()` is not const — and not a state any caller
// should observe. Building on first use rather than unwrapping means a call
// that runs before `init()` gets a working table instead of panicking.
fn with_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut RadioTable) -> R,
{
    let mut guard = TABLE.lock();
    f(guard.get_or_insert_with(RadioTable::new))
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

/// True if `channel` is a channel number this medium will tune to.
///
/// 2.4 GHz channels 1–14 and the 20 MHz 5 GHz channels. Channel 14 is
/// Japan-only in reality; regulatory domains are not modelled here, and a
/// simulated radio has no regulator.
#[must_use]
pub fn channel_is_valid(channel: u8) -> bool {
    (1..=14).contains(&channel) || CHANNELS_5GHZ.contains(&channel)
}

// ---------------------------------------------------------------------------
// MAC generation
// ---------------------------------------------------------------------------

/// Generate a locally-administered unicast MAC for a simulated radio.
///
/// Format `02:57:00:ii:ss:SS` — `02` is locally-administered unicast (bit 1
/// set, bit 0 clear, so it is never mistaken for a group address), `57` is
/// ASCII `W`, `ii` is the radio index and `ss:SS` a sequence counter.
#[allow(clippy::cast_possible_truncation)]
fn generate_mac(index: usize, seq: u32) -> MacAddr {
    [
        0x02,
        0x57,
        0x00,
        (index & 0xFF) as u8,
        (seq & 0xFF) as u8,
        ((seq >> 8) & 0xFF) as u8,
    ]
}

// ---------------------------------------------------------------------------
// Public API: lifecycle
// ---------------------------------------------------------------------------

/// Create a simulated radio.
///
/// The radio starts **down**, tuned to [`DEFAULT_CHANNEL`], with no keys. Use
/// [`set_up`] to bring it up.
///
/// # Errors
///
/// - [`KernelError::ResourceExhausted`] if all [`MAX_RADIOS`] slots are in use.
pub fn create_radio() -> KernelResult<RadioId> {
    with_table(|table| {
        let idx = table
            .radios
            .iter()
            .position(|r| !r.active)
            .ok_or(KernelError::ResourceExhausted)?;
        let seq = table.mac_counter;
        table.mac_counter = table.mac_counter.wrapping_add(1);
        let mac = generate_mac(idx, seq);
        let radio = table
            .radios
            .get_mut(idx)
            .ok_or(KernelError::InternalError)?;
        *radio = Radio::empty();
        radio.active = true;
        radio.mac = mac;
        Ok(idx)
    })
}

/// Destroy a simulated radio, discarding anything still in its RX queue.
///
/// # Errors
///
/// - [`KernelError::NoSuchDevice`] if `id` is not an active radio.
pub fn destroy_radio(id: RadioId) -> KernelResult<()> {
    with_table(|table| {
        let radio = table.radios.get_mut(id).ok_or(KernelError::NoSuchDevice)?;
        if !radio.active {
            return Err(KernelError::NoSuchDevice);
        }
        *radio = Radio::empty();
        Ok(())
    })
}

/// Bring a radio up or down.
///
/// # Errors
///
/// - [`KernelError::NoSuchDevice`] if `id` is not an active radio.
pub fn set_up(id: RadioId, up: bool) -> KernelResult<()> {
    with_table(|table| {
        let radio = table.radios.get_mut(id).ok_or(KernelError::NoSuchDevice)?;
        if !radio.active {
            return Err(KernelError::NoSuchDevice);
        }
        radio.up = up;
        Ok(())
    })
}

/// Enable or disable monitor mode (accept every frame on the channel).
///
/// # Errors
///
/// - [`KernelError::NoSuchDevice`] if `id` is not an active radio.
pub fn set_promiscuous(id: RadioId, on: bool) -> KernelResult<()> {
    with_table(|table| {
        let radio = table.radios.get_mut(id).ok_or(KernelError::NoSuchDevice)?;
        if !radio.active {
            return Err(KernelError::NoSuchDevice);
        }
        radio.promiscuous = on;
        Ok(())
    })
}

/// Tune a radio to `channel`, returning the channel it is now on.
///
/// The return value is the confirmation the driver boundary asks for: a caller
/// that sets a channel learns what it actually got rather than assuming the
/// request took effect.
///
/// # Errors
///
/// - [`KernelError::NoSuchDevice`] if `id` is not an active radio.
/// - [`KernelError::InvalidArgument`] if `channel` is not a real channel
///   number — see [`channel_is_valid`].
pub fn set_channel(id: RadioId, channel: u8) -> KernelResult<u8> {
    if !channel_is_valid(channel) {
        return Err(KernelError::InvalidArgument);
    }
    with_table(|table| {
        let radio = table.radios.get_mut(id).ok_or(KernelError::NoSuchDevice)?;
        if !radio.active {
            return Err(KernelError::NoSuchDevice);
        }
        radio.channel = channel;
        Ok(radio.channel)
    })
}

/// The channel a radio is tuned to, or `None` if `id` is not active.
#[must_use]
pub fn channel(id: RadioId) -> Option<u8> {
    with_table(|table| {
        let radio = table.radios.get(id)?;
        radio.active.then_some(radio.channel)
    })
}

/// A radio's MAC address, or `None` if `id` is not active.
#[must_use]
pub fn mac(id: RadioId) -> Option<MacAddr> {
    with_table(|table| {
        let radio = table.radios.get(id)?;
        radio.active.then_some(radio.mac)
    })
}

// ---------------------------------------------------------------------------
// Public API: the medium
// ---------------------------------------------------------------------------

/// Transmit `frame` from radio `id` onto the medium.
///
/// The frame is offered to every *other* radio that is up and on the same
/// channel; each applies its own address filter. Returns the number of radios
/// that queued it — zero is a normal outcome (nobody is listening on this
/// channel, or the frame was addressed to someone else) and not an error, which
/// is the same thing a real radio reports for an unacknowledged transmission.
///
/// Frames rejected by a receiver's filter, or dropped because its queue was
/// full, are counted on *that* receiver: see [`RadioStats::rx_filtered`] and
/// [`RadioStats::rx_dropped_full`].
///
/// # Errors
///
/// - [`KernelError::NoSuchDevice`] if `id` is not an active radio.
/// - [`KernelError::NotConnected`] if the radio is down.
/// - [`KernelError::MessageTooLarge`] if the frame exceeds [`MAX_FRAME_LEN`].
/// - [`KernelError::InvalidArgument`] if the buffer is not a well-formed
///   802.11 MAC header. A radio cannot transmit something that is not a frame,
///   and refusing here is what makes "everything on this medium is a frame"
///   true for anything reading it.
pub fn transmit(id: RadioId, frame: &[u8]) -> KernelResult<usize> {
    if frame.len() > MAX_FRAME_LEN {
        return Err(KernelError::MessageTooLarge);
    }
    let header = MacHeader::parse(frame).ok_or(KernelError::InvalidArgument)?;
    let addr1 = header.addr1;
    let protected = header.fc.protected();

    with_table(|table| {
        let sender_channel = {
            let radio = table.radios.get_mut(id).ok_or(KernelError::NoSuchDevice)?;
            if !radio.active {
                return Err(KernelError::NoSuchDevice);
            }
            if !radio.up {
                return Err(KernelError::NotConnected);
            }
            radio.tx_frames = radio.tx_frames.saturating_add(1);
            radio.tx_bytes = radio.tx_bytes.saturating_add(frame.len() as u64);
            // The packet number advances only for a frame that says it is
            // protected, and only when there is a key for it to be protected
            // with. Counting every frame would make the PN meaningless as
            // evidence about key installs, which is the only reason it is here.
            if protected && radio.pairwise.is_some() {
                radio.pairwise_pn = radio.pairwise_pn.saturating_add(1);
            }
            radio.channel
        };

        let mut delivered = 0usize;
        for (other_id, other) in table.radios.iter_mut().enumerate() {
            if other_id == id || !other.active || !other.up {
                continue;
            }
            if other.channel != sender_channel {
                continue;
            }
            // A radio never hears itself. The identity check above is by slot
            // rather than by address, so a test that deliberately gives two
            // radios the same MAC still gets a frame delivered to the second
            // one instead of having the duplicate silently swallowed here.
            if other.offer(&addr1, frame) {
                delivered = delivered.saturating_add(1);
            }
        }
        Ok(delivered)
    })
}

/// Take the next frame from a radio's RX queue, oldest first.
///
/// Returns `None` if the queue is empty or `id` is not an active radio.
#[must_use]
pub fn receive(id: RadioId) -> Option<Vec<u8>> {
    with_table(|table| {
        let radio = table.radios.get_mut(id)?;
        if !radio.active {
            return None;
        }
        radio.rx_queue.pop_front()
    })
}

/// How many frames are waiting on a radio's RX queue.
#[must_use]
pub fn rx_pending(id: RadioId) -> usize {
    with_table(|table| {
        table
            .radios
            .get(id)
            .filter(|r| r.active)
            .map_or(0, |r| r.rx_queue.len())
    })
}

// ---------------------------------------------------------------------------
// Public API: keys
// ---------------------------------------------------------------------------

/// Install the pairwise key, resetting the packet number.
///
/// **Installing the same key twice is refused**, because installing a key
/// resets the CCMP packet number and a repeated nonce leaks keystream — this is
/// KRACK. `net80211::supplicant` is built so a caller that matches on
/// `Outcome` cannot reach the reinstall path: only `Outcome::Complete` means
/// "install", and a retransmitted message 3 yields `Outcome::Retransmission`.
/// This check exists so that a caller who gets that wrong finds out here.
///
/// A refused reinstall does **not** touch the packet number: the point of
/// refusing is that the nonce space is not rewound.
///
/// # Errors
///
/// - [`KernelError::NoSuchDevice`] if `id` is not an active radio.
/// - [`KernelError::InvalidArgument`] if the key is empty or longer than 32
///   octets (the largest key any cipher in `net80211` uses).
/// - [`KernelError::AlreadyExists`] if this exact key is already installed.
pub fn install_pairwise_key(id: RadioId, key: &[u8]) -> KernelResult<()> {
    install_key(id, KeyKind::Pairwise, 0, key)
}

/// Install a group key in slot `key_id`, under the same reinstall rule as
/// [`install_pairwise_key`].
///
/// # Errors
///
/// Same as [`install_pairwise_key`], plus [`KernelError::InvalidArgument`] if
/// `key_id` is not 0–3 (IEEE 802.11-2020 §12.5.1 defines four key slots).
pub fn install_group_key(id: RadioId, key_id: u8, key: &[u8]) -> KernelResult<()> {
    if key_id > 3 {
        return Err(KernelError::InvalidArgument);
    }
    install_key(id, KeyKind::Group, key_id, key)
}

/// The largest key any cipher in `net80211` uses (a 256-bit GTK).
const MAX_KEY_LEN: usize = 32;

fn install_key(id: RadioId, kind: KeyKind, key_id: u8, key: &[u8]) -> KernelResult<()> {
    if key.is_empty() || key.len() > MAX_KEY_LEN {
        return Err(KernelError::InvalidArgument);
    }
    with_table(|table| {
        let radio = table.radios.get_mut(id).ok_or(KernelError::NoSuchDevice)?;
        if !radio.active {
            return Err(KernelError::NoSuchDevice);
        }
        let slot = match kind {
            KeyKind::Pairwise => &mut radio.pairwise,
            KeyKind::Group => &mut radio.group,
        };
        if let Some(existing) = slot.as_ref() {
            if existing.key_id == key_id && existing.key.as_slice() == key {
                radio.key_reinstalls_refused = radio.key_reinstalls_refused.saturating_add(1);
                return Err(KernelError::AlreadyExists);
            }
        }
        let installs = slot.as_ref().map_or(0, |k| k.installs).saturating_add(1);
        *slot = Some(InstalledKey {
            kind,
            key_id,
            key: key.to_vec(),
            installs,
        });
        if kind == KeyKind::Pairwise {
            // A genuinely new key starts a fresh nonce space. This is the
            // behaviour that makes a *refused* reinstall meaningful: the two
            // paths differ in exactly this line.
            radio.pairwise_pn = 0;
        }
        Ok(())
    })
}

/// The current pairwise packet number, or `None` if `id` is not active.
#[must_use]
pub fn pairwise_pn(id: RadioId) -> Option<u64> {
    with_table(|table| {
        let radio = table.radios.get(id)?;
        radio.active.then_some(radio.pairwise_pn)
    })
}

// ---------------------------------------------------------------------------
// Public API: introspection
// ---------------------------------------------------------------------------

fn radio_stats(id: RadioId, r: &Radio) -> RadioStats {
    RadioStats {
        id,
        mac: r.mac,
        channel: r.channel,
        up: r.up,
        promiscuous: r.promiscuous,
        pairwise_installed: r.pairwise.is_some(),
        group_installed: r.group.is_some(),
        pairwise_installs: r.pairwise.as_ref().map_or(0, |k| k.installs),
        pairwise_pn: r.pairwise_pn,
        key_reinstalls_refused: r.key_reinstalls_refused,
        tx_frames: r.tx_frames,
        tx_bytes: r.tx_bytes,
        rx_frames: r.rx_frames,
        rx_bytes: r.rx_bytes,
        rx_dropped_full: r.rx_dropped_full,
        rx_filtered: r.rx_filtered,
        rx_pending: r.rx_queue.len(),
    }
}

/// Counters for one radio, or `None` if `id` is not active.
#[must_use]
pub fn stats(id: RadioId) -> Option<RadioStats> {
    with_table(|table| {
        let radio = table.radios.get(id)?;
        radio.active.then(|| radio_stats(id, radio))
    })
}

/// Counters for every active radio.
#[must_use]
pub fn list_all() -> Vec<RadioStats> {
    with_table(|table| {
        let mut out = Vec::new();
        for (i, radio) in table.radios.iter().enumerate() {
            if radio.active {
                out.push(radio_stats(i, radio));
            }
        }
        out
    })
}

/// How many radios currently exist.
#[must_use]
pub fn active_count() -> usize {
    with_table(|table| table.radios.iter().filter(|r| r.active).count())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

use net80211::frame::{FrameControl, FrameType, SeqCtrl, data_subtype};

/// The length of the 3-address data header the tests below build.
const TEST_HEADER_LEN: usize = 24;

/// Build a well-formed 3-address data frame for the tests.
///
/// Deliberately built with `net80211`'s own writer rather than by hand: a
/// hand-rolled byte string would let this module and the frame layer drift
/// apart while every test still passed.
fn test_data_frame(
    from: MacAddr,
    to: MacAddr,
    bssid: MacAddr,
    protected: bool,
    body_len: usize,
) -> KernelResult<Vec<u8>> {
    let fc = FrameControl::new(FrameType::Data, data_subtype::DATA).with_protected(protected);
    let header = MacHeader {
        fc,
        duration_id: 0,
        addr1: to,
        addr2: Some(from),
        addr3: Some(bssid),
        seq_ctrl: Some(SeqCtrl::new(0, 0)),
        addr4: None,
        qos_ctrl: None,
        ht_ctrl: None,
        header_len: TEST_HEADER_LEN,
    };
    let total = TEST_HEADER_LEN
        .checked_add(body_len)
        .ok_or(KernelError::InvalidArgument)?;
    let mut buf = alloc::vec![0u8; total];
    let written = header.write(&mut buf).ok_or(KernelError::InternalError)?;
    if written != TEST_HEADER_LEN {
        crate::serial_println!("[hwsim]   FAIL: header wrote {written} octets, expected 24");
        return Err(KernelError::InternalError);
    }
    for (i, byte) in buf.iter_mut().skip(TEST_HEADER_LEN).enumerate() {
        *byte = u8::try_from(i & 0xFF).unwrap_or(0);
    }
    Ok(buf)
}

/// Report a failed expectation and produce the error every test returns.
fn fail(what: &str) -> KernelError {
    crate::serial_println!("[hwsim]   FAIL: {what}");
    KernelError::InternalError
}

/// Create a radio that is up and on `channel`.
fn radio_on(channel: u8) -> KernelResult<RadioId> {
    let id = create_radio()?;
    set_channel(id, channel)?;
    set_up(id, true)?;
    Ok(id)
}

/// Comprehensive self-test for the simulated-radio subsystem.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] on the first failed expectation,
/// having printed which one.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[hwsim] Running self-test...");

    test_lifecycle()?;
    test_mac_generation()?;
    test_channel_validation()?;
    test_frame_crosses_the_medium()?;
    test_channel_isolates()?;
    test_a_down_radio_is_deaf_and_mute()?;
    test_address_filter()?;
    test_promiscuous_hears_everything()?;
    test_queue_full_drops_the_newest()?;
    test_medium_carries_only_frames()?;
    test_key_reinstall_is_refused_and_does_not_rewind_the_nonce()?;
    test_group_key_slot_is_independent()?;

    crate::serial_println!("[hwsim] Self-test PASSED (12 tests)");
    Ok(())
}

/// Test 1: create, destroy, and the errors either side of them.
fn test_lifecycle() -> KernelResult<()> {
    let before = active_count();
    let id = create_radio()?;
    if active_count() != before.saturating_add(1) {
        return Err(fail("active_count did not rise after create"));
    }
    if stats(id).is_none() {
        return Err(fail("stats missing for a radio just created"));
    }
    // A new radio is down, so that a test cannot accidentally transmit before
    // it has set a channel.
    if stats(id).is_some_and(|s| s.up) {
        return Err(fail("a new radio should start down"));
    }
    destroy_radio(id)?;
    if active_count() != before {
        return Err(fail("active_count did not fall after destroy"));
    }
    if stats(id).is_some() {
        return Err(fail("stats present for a destroyed radio"));
    }
    if destroy_radio(id) != Err(KernelError::NoSuchDevice) {
        return Err(fail("double destroy should be NoSuchDevice"));
    }
    if set_up(id, true) != Err(KernelError::NoSuchDevice) {
        return Err(fail("set_up on a destroyed radio should be NoSuchDevice"));
    }
    if set_up(MAX_RADIOS.saturating_add(99), true) != Err(KernelError::NoSuchDevice) {
        return Err(fail("set_up past the table should be NoSuchDevice"));
    }
    crate::serial_println!("[hwsim]   test 1 (lifecycle): OK");
    Ok(())
}

/// Test 2: MACs are locally-administered unicast, and distinct.
fn test_mac_generation() -> KernelResult<()> {
    let a = create_radio()?;
    let b = create_radio()?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for radio a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for radio b"))?;

    if mac_a[0] & 0x02 == 0 {
        return Err(fail("MAC is not locally administered"));
    }
    // Bit 0 clear: a group-addressed source would make the address filter
    // accept the sender's own traffic everywhere, which would hide every
    // filtering bug this module could have.
    if mac_a[0] & 0x01 != 0 {
        return Err(fail("MAC is a group address"));
    }
    if is_group_addr(&mac_a) {
        return Err(fail("is_group_addr disagrees with the bit-0 check"));
    }
    if mac_a == mac_b {
        return Err(fail("two radios share a MAC"));
    }

    // A recreated radio must not reuse the address a peer may have cached.
    destroy_radio(a)?;
    let c = create_radio()?;
    let mac_c = mac(c).ok_or_else(|| fail("no MAC for the recreated radio"))?;
    if mac_c == mac_a {
        return Err(fail("a recreated radio reused the destroyed radio's MAC"));
    }

    destroy_radio(b)?;
    destroy_radio(c)?;
    crate::serial_println!("[hwsim]   test 2 (MAC generation): OK");
    Ok(())
}

/// Test 3: channel numbers are validated, and a set is confirmed.
fn test_channel_validation() -> KernelResult<()> {
    for good in [1u8, 6, 11, 14, 36, 100, 165] {
        if !channel_is_valid(good) {
            return Err(fail("a real channel was rejected"));
        }
    }
    // 0 and 15 do not exist in 2.4 GHz; 37 and 166 fall in the gaps between
    // 20 MHz 5 GHz channels.
    for bad in [0u8, 15, 35, 37, 166, 255] {
        if channel_is_valid(bad) {
            return Err(fail("a channel that does not exist was accepted"));
        }
    }

    let id = create_radio()?;
    if channel(id) != Some(DEFAULT_CHANNEL) {
        return Err(fail("a new radio is not on the default channel"));
    }
    if set_channel(id, 36)? != 36 {
        return Err(fail("set_channel did not confirm the channel it set"));
    }
    if channel(id) != Some(36) {
        return Err(fail("the channel did not stick"));
    }
    if set_channel(id, 37) != Err(KernelError::InvalidArgument) {
        return Err(fail("set_channel accepted a channel that does not exist"));
    }
    if channel(id) != Some(36) {
        return Err(fail("a refused set_channel changed the channel anyway"));
    }
    destroy_radio(id)?;
    crate::serial_println!("[hwsim]   test 3 (channel validation): OK");
    Ok(())
}

/// Test 4: a frame transmitted on one radio arrives, intact, on another.
fn test_frame_crosses_the_medium() -> KernelResult<()> {
    let a = radio_on(6)?;
    let b = radio_on(6)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;

    let frame = test_data_frame(mac_a, mac_b, mac_a, false, 64)?;
    if transmit(a, &frame)? != 1 {
        return Err(fail("frame was not delivered to exactly one radio"));
    }
    let got = receive(b).ok_or_else(|| fail("b received nothing"))?;
    if got != frame {
        return Err(fail(
            "the frame that arrived is not the frame that was sent",
        ));
    }
    // The transmitter does not hear itself.
    if receive(a).is_some() {
        return Err(fail("a radio heard its own transmission"));
    }

    let sa = stats(a).ok_or_else(|| fail("no stats for a"))?;
    let sb = stats(b).ok_or_else(|| fail("no stats for b"))?;
    if sa.tx_frames != 1 || sa.tx_bytes != frame.len() as u64 {
        return Err(fail("TX counters wrong"));
    }
    if sb.rx_frames != 1 || sb.rx_bytes != frame.len() as u64 {
        return Err(fail("RX counters wrong"));
    }
    if sb.rx_pending != 0 {
        return Err(fail("a drained queue still reports frames pending"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    crate::serial_println!("[hwsim]   test 4 (frame crosses the medium): OK");
    Ok(())
}

/// Test 5: radios on different channels cannot hear each other.
fn test_channel_isolates() -> KernelResult<()> {
    let a = radio_on(1)?;
    let b = radio_on(11)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;

    let frame = test_data_frame(mac_a, mac_b, mac_a, false, 16)?;
    if transmit(a, &frame)? != 0 {
        return Err(fail("a frame reached a radio on another channel"));
    }
    if receive(b).is_some() {
        return Err(fail("b received a frame from another channel"));
    }
    // A frame nobody is on-channel for is not a *filtered* frame: the receiver
    // never saw it at all, and counting it as filtered would make the counter
    // read as an addressing problem.
    if stats(b).is_some_and(|s| s.rx_filtered != 0) {
        return Err(fail("an off-channel frame was counted as filtered"));
    }

    // Retuning is enough to join the conversation.
    set_channel(b, 1)?;
    if transmit(a, &frame)? != 1 {
        return Err(fail("retuning to the sender's channel did not help"));
    }
    if receive(b).is_none() {
        return Err(fail("b heard nothing after retuning"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    crate::serial_println!("[hwsim]   test 5 (channel isolates): OK");
    Ok(())
}

/// Test 6: a radio that is down neither transmits nor receives.
fn test_a_down_radio_is_deaf_and_mute() -> KernelResult<()> {
    let a = radio_on(6)?;
    let b = radio_on(6)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;
    let frame = test_data_frame(mac_a, mac_b, mac_a, false, 16)?;

    set_up(b, false)?;
    if transmit(a, &frame)? != 0 {
        return Err(fail("a down radio received a frame"));
    }
    if receive(b).is_some() {
        return Err(fail("a down radio queued a frame"));
    }

    set_up(a, false)?;
    if transmit(a, &frame) != Err(KernelError::NotConnected) {
        return Err(fail("a down radio was allowed to transmit"));
    }
    if stats(a).is_some_and(|s| s.tx_frames != 1) {
        return Err(fail("a refused transmit was counted as a transmission"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    crate::serial_println!("[hwsim]   test 6 (a down radio is deaf and mute): OK");
    Ok(())
}

/// Test 7: the address filter accepts what is addressed here, and broadcasts.
fn test_address_filter() -> KernelResult<()> {
    let a = radio_on(6)?;
    let b = radio_on(6)?;
    let c = radio_on(6)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;

    // Addressed to b: c hears the transmission but rejects it.
    let unicast = test_data_frame(mac_a, mac_b, mac_a, false, 16)?;
    if transmit(a, &unicast)? != 1 {
        return Err(fail(
            "a unicast frame was delivered to more than its target",
        ));
    }
    if receive(b).is_none() {
        return Err(fail("the addressed radio did not receive"));
    }
    if receive(c).is_some() {
        return Err(fail("a radio received a frame addressed elsewhere"));
    }
    // Counted, not silently dropped: this is the difference between "filtered"
    // and "lost", and only one of them is a bug when a test goes looking.
    if stats(c).is_none_or(|s| s.rx_filtered != 1) {
        return Err(fail("a filtered frame was not counted"));
    }

    // Broadcast reaches both.
    let bcast = test_data_frame(mac_a, net80211::BROADCAST_MAC, mac_a, false, 16)?;
    if transmit(a, &bcast)? != 2 {
        return Err(fail("a broadcast did not reach both radios"));
    }
    if receive(b).is_none() || receive(c).is_none() {
        return Err(fail("a broadcast was not queued on both radios"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    destroy_radio(c)?;
    crate::serial_println!("[hwsim]   test 7 (address filter): OK");
    Ok(())
}

/// Test 8: monitor mode disables the address filter.
fn test_promiscuous_hears_everything() -> KernelResult<()> {
    let a = radio_on(6)?;
    let b = radio_on(6)?;
    let sniffer = radio_on(6)?;
    set_promiscuous(sniffer, true)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;

    let frame = test_data_frame(mac_a, mac_b, mac_a, false, 16)?;
    if transmit(a, &frame)? != 2 {
        return Err(fail(
            "the monitor did not receive a frame addressed elsewhere",
        ));
    }
    if receive(sniffer).is_none() {
        return Err(fail("the monitor queued nothing"));
    }
    if stats(sniffer).is_some_and(|s| s.rx_filtered != 0) {
        return Err(fail("a monitor filtered a frame"));
    }

    // Turning it off restores the filter.
    set_promiscuous(sniffer, false)?;
    if transmit(a, &frame)? != 1 {
        return Err(fail("the filter did not come back"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    destroy_radio(sniffer)?;
    crate::serial_println!("[hwsim]   test 8 (promiscuous hears everything): OK");
    Ok(())
}

/// Test 9: a full queue drops the newest frame and keeps the order of the rest.
fn test_queue_full_drops_the_newest() -> KernelResult<()> {
    let a = radio_on(6)?;
    let b = radio_on(6)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;

    // One more than the queue holds. Each frame's body carries its index, so a
    // reorder is visible rather than merely a count being wrong.
    for i in 0..RX_QUEUE_DEPTH.saturating_add(1) {
        let mut frame = test_data_frame(mac_a, mac_b, mac_a, false, 4)?;
        let marker = u8::try_from(i & 0xFF).unwrap_or(0);
        if let Some(byte) = frame.get_mut(TEST_HEADER_LEN) {
            *byte = marker;
        } else {
            return Err(fail("the test frame has no body to mark"));
        }
        let delivered = transmit(a, &frame)?;
        let expected = usize::from(i < RX_QUEUE_DEPTH);
        if delivered != expected {
            return Err(fail("delivery count wrong across the queue bound"));
        }
    }

    if !stats(b).is_some_and(|s| s.rx_dropped_full == 1 && s.rx_pending == RX_QUEUE_DEPTH) {
        return Err(fail("the overflow was not counted as a full-queue drop"));
    }

    // Drain and check the order: the frames kept are the first RX_QUEUE_DEPTH,
    // in the order they were sent.
    for i in 0..RX_QUEUE_DEPTH {
        let got = receive(b).ok_or_else(|| fail("the queue emptied early"))?;
        let marker = got.get(TEST_HEADER_LEN).copied();
        if marker != Some(u8::try_from(i & 0xFF).unwrap_or(0)) {
            return Err(fail("frames were reordered or the wrong one was dropped"));
        }
    }
    if receive(b).is_some() {
        return Err(fail("more frames were queued than the bound allows"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    crate::serial_println!("[hwsim]   test 9 (queue-full drops the newest): OK");
    Ok(())
}

/// Test 10: the medium refuses anything that is not a well-formed frame.
fn test_medium_carries_only_frames() -> KernelResult<()> {
    let a = radio_on(6)?;
    let b = radio_on(6)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;

    // Too short to hold even Frame Control, Duration and Address 1.
    if transmit(a, &[0x08, 0x00, 0x00]) != Err(KernelError::InvalidArgument) {
        return Err(fail("a runt was accepted onto the medium"));
    }
    if transmit(a, &[]) != Err(KernelError::InvalidArgument) {
        return Err(fail("an empty buffer was accepted onto the medium"));
    }
    // A data frame header that stops before Address 2.
    if transmit(a, &[0x08, 0x00, 0, 0, 1, 2, 3, 4, 5, 6, 0xAA]) != Err(KernelError::InvalidArgument)
    {
        return Err(fail("a truncated header was accepted onto the medium"));
    }

    let oversize = test_data_frame(mac_a, mac_b, mac_a, false, MAX_FRAME_LEN)?;
    if transmit(a, &oversize) != Err(KernelError::MessageTooLarge) {
        return Err(fail("an oversize frame was accepted"));
    }
    // The largest frame that *is* allowed still goes through, so the bound is
    // not off by one in the safe direction either.
    let biggest = test_data_frame(
        mac_a,
        mac_b,
        mac_a,
        false,
        MAX_FRAME_LEN.saturating_sub(TEST_HEADER_LEN),
    )?;
    if biggest.len() != MAX_FRAME_LEN {
        return Err(fail("the largest allowed frame is not MAX_FRAME_LEN long"));
    }
    if transmit(a, &biggest)? != 1 {
        return Err(fail("the largest allowed frame was refused"));
    }
    if receive(b).is_none() {
        return Err(fail("the largest allowed frame did not arrive"));
    }
    if stats(a).is_some_and(|s| s.tx_frames != 1) {
        return Err(fail("a refused frame was counted as transmitted"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    crate::serial_println!("[hwsim]   test 10 (medium carries only frames): OK");
    Ok(())
}

/// Test 11: the KRACK guard.
///
/// Installing a key resets the CCMP packet number; installing the *same* key
/// again would rewind the nonce space and leak keystream. The refusal, the
/// count, and — the part that actually matters — the packet number *not*
/// moving are all checked separately, because a guard that refuses but rewinds
/// anyway would pass a test that only looked at the error code.
fn test_key_reinstall_is_refused_and_does_not_rewind_the_nonce() -> KernelResult<()> {
    let a = radio_on(6)?;
    let b = radio_on(6)?;
    let mac_a = mac(a).ok_or_else(|| fail("no MAC for a"))?;
    let mac_b = mac(b).ok_or_else(|| fail("no MAC for b"))?;

    let ptk = [0x11u8; 16];
    let other_ptk = [0x22u8; 16];

    if pairwise_pn(a) != Some(0) {
        return Err(fail("a new radio does not start at packet number 0"));
    }
    // Before a key exists, a protected frame does not advance the PN: there is
    // no nonce space to consume yet.
    let protected = test_data_frame(mac_a, mac_b, mac_a, true, 16)?;
    transmit(a, &protected)?;
    if pairwise_pn(a) != Some(0) {
        return Err(fail("the packet number moved with no key installed"));
    }
    let _ = receive(b);

    install_pairwise_key(a, &ptk)?;
    if !stats(a).is_some_and(|s| s.pairwise_installed && s.pairwise_installs == 1) {
        return Err(fail("the key was not recorded as installed"));
    }

    // Three protected frames: the PN advances once per frame.
    for _ in 0..3 {
        transmit(a, &protected)?;
        let _ = receive(b);
    }
    if pairwise_pn(a) != Some(3) {
        return Err(fail(
            "the packet number did not advance once per protected frame",
        ));
    }
    // An unprotected frame does not consume nonce space.
    let clear = test_data_frame(mac_a, mac_b, mac_a, false, 16)?;
    transmit(a, &clear)?;
    let _ = receive(b);
    if pairwise_pn(a) != Some(3) {
        return Err(fail("an unprotected frame advanced the packet number"));
    }

    // The reinstall. This is the KRACK case.
    if install_pairwise_key(a, &ptk) != Err(KernelError::AlreadyExists) {
        return Err(fail("reinstalling an identical key was allowed"));
    }
    if pairwise_pn(a) != Some(3) {
        return Err(fail("a refused reinstall rewound the packet number anyway"));
    }
    if !stats(a).is_some_and(|s| s.key_reinstalls_refused == 1 && s.pairwise_installs == 1) {
        return Err(fail(
            "the refused reinstall was not counted, or counted as an install",
        ));
    }

    // A genuinely different key is a rekey, and does reset the nonce space.
    install_pairwise_key(a, &other_ptk)?;
    if pairwise_pn(a) != Some(0) {
        return Err(fail("a real rekey did not reset the packet number"));
    }
    if stats(a).is_none_or(|s| s.pairwise_installs != 2) {
        return Err(fail("a real rekey was not counted as an install"));
    }

    // Bad key material is refused before any of that.
    if install_pairwise_key(a, &[]) != Err(KernelError::InvalidArgument) {
        return Err(fail("an empty key was accepted"));
    }
    if install_pairwise_key(a, &[0u8; 64]) != Err(KernelError::InvalidArgument) {
        return Err(fail("an over-long key was accepted"));
    }
    if pairwise_pn(a) != Some(0) {
        return Err(fail("a refused key install disturbed the packet number"));
    }

    destroy_radio(a)?;
    destroy_radio(b)?;
    crate::serial_println!("[hwsim]   test 11 (key reinstall refused, nonce not rewound): OK");
    Ok(())
}

/// Test 12: the group key slot is separate from the pairwise one.
fn test_group_key_slot_is_independent() -> KernelResult<()> {
    let a = create_radio()?;
    let key = [0x33u8; 16];

    if install_group_key(a, 4, &key) != Err(KernelError::InvalidArgument) {
        return Err(fail("a key index outside 0-3 was accepted"));
    }
    install_group_key(a, 1, &key)?;
    if !stats(a).is_some_and(|s| s.group_installed && !s.pairwise_installed) {
        return Err(fail("a group key install touched the pairwise slot"));
    }
    // The same bytes are a distinct key in a distinct slot: refusing this would
    // make a rekey to a different index impossible.
    install_group_key(a, 2, &key)?;
    // ...but the same bytes in the same slot is still a reinstall.
    if install_group_key(a, 2, &key) != Err(KernelError::AlreadyExists) {
        return Err(fail("an identical group-key reinstall was allowed"));
    }
    // Installing the same bytes as the pairwise key is fine — different slot.
    install_pairwise_key(a, &key)?;
    if !stats(a).is_some_and(|s| s.pairwise_installed && s.group_installed) {
        return Err(fail("both slots should now hold a key"));
    }
    if stats(a).is_none_or(|s| s.key_reinstalls_refused != 1) {
        return Err(fail("the group reinstall refusal was not counted"));
    }

    destroy_radio(a)?;
    crate::serial_println!("[hwsim]   test 12 (group key slot is independent): OK");
    Ok(())
}
