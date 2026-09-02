//! The station-side association driver: the outer loop that turns a chosen
//! BSS into an encrypted link.
//!
//! [`supplicant`](crate::supplicant) knows how to *parse and build* every
//! frame an association needs, and how to run the 4-way handshake once its
//! frames are handed to it. What it does not know is the order they go in,
//! when a key becomes safe to install, or how to get a frame onto the air.
//! This module knows the first two and delegates the third to a
//! [`Transceiver`].
//!
//! ## Why the radio is a trait and not a caller-supplied pump
//!
//! The alternative was for each driver — the simulated radio in the kernel's
//! test harness, and later a real chipset — to write its own loop calling
//! into `supplicant` in the right order. That loop is not glue: it *is* the
//! association state machine's outer half, and duplicating it means every
//! future change to the handshake's shape has to be made once per driver, by
//! whoever owns that driver's tree. Behind a trait it is written once, tested
//! here against a mock with no hardware in it, and a new driver is one `impl`
//! with an existing working example to copy.
//!
//! ## No clock
//!
//! `net80211` has no time source and is not given one. [`Association::poll`]
//! is a *step function*: it does at most one thing per call and reports what
//! it did. The caller owns the loop, the retry timing and the bound — which
//! is also what makes the whole association testable here with no timer and
//! no scheduler, by calling `poll` until it says the link is up.
//!
//! [`Association::retransmit`] exists for the same reason: on a lossy medium
//! an authentication or association request can be lost, and the decision
//! *when* to resend it is a timing decision, so it belongs to the caller.
//!
//! ## What this module does not do
//!
//! - **It does not scan.** Choosing a BSS is policy — which band, which
//!   signal strength, whether to probe for a hidden SSID, whether to prefer
//!   a remembered network — and it is made once, before any of this runs.
//!   The caller passes [`supplicant::scan`] a beacon it captured and hands
//!   the result in as a [`supplicant::Config`].
//! - **It does not encrypt.** Setting the Protected Frame bit is this
//!   module's job; putting CCMP under it is the driver's or the hardware's.
//!   A link this module reports as established has the *keys installed on
//!   both sides* — which is not the same claim as confidentiality, and must
//!   not be reported as if it were.
//! - **It does not roam or rekey the PMK.** A deauthentication ends the
//!   association; starting a new one is a new [`Association`].

use crate::eapol::NONCE_LEN;
use crate::frame::{self, FrameType};
use crate::llc;
use crate::mgmt;
use crate::supplicant::{self, Config, Handshake, PMK_LEN};
use crate::{MAX_SSID_LEN, MacAddr};

/// The largest 802.11 frame this driver will handle, excluding the FCS.
///
/// A 2304-octet MSDU plus a four-address QoS+HT header and the CCMP header
/// and MIC. Sized so that a conforming frame is never refused for length; a
/// longer one is not a frame this stack can have produced or should accept.
pub const MAX_FRAME_LEN: usize = 2352;

/// The largest Ethernet II frame a conforming 802.11 data frame can become.
///
/// The 2304-octet MSDU limit, less the eight-octet LLC/SNAP shim, plus the
/// fourteen-octet Ethernet header.
pub const MAX_ETHERNET_LEN: usize = 2310;

/// The largest EAPOL-Key frame this module builds.
///
/// The header, the fixed fields, a 24-octet MIC and a full 255-octet RSN
/// element in Key Data come to under 400; 512 leaves room without being
/// large enough to matter.
pub const MAX_EAPOL_LEN: usize = 512;

/// The most rates a Supported Rates element can carry (§9.4.2.3).
pub const MAX_SUPPORTED_RATES: usize = 8;

/// A conventional Supported Rates element body for a 2.4 GHz BSS: 1, 2, 5.5
/// and 11 Mb/s marked basic, then 6, 12, 24 and 36 Mb/s.
///
/// Units are 500 kb/s, and the top bit marks a rate the BSS requires all
/// members to support. Offered as a default because an association request
/// must carry *something* and this is what every station sends; a driver that
/// knows its radio's real capabilities should send those instead.
pub const BASIC_RATES: [u8; 8] = [0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x24];

// ---------------------------------------------------------------------------
// The radio
// ---------------------------------------------------------------------------

/// The next frame did not fit the buffer offered for it.
///
/// A [`Transceiver`] must never answer an undersized buffer by copying part
/// of the frame: a truncated 802.11 frame parses as a *different*, shorter,
/// still-well-formed frame, which is the worst available failure mode — the
/// caller cannot tell it happened, and the frame it acts on is not the frame
/// that arrived. Return this instead, and say how long the frame was so the
/// caller can tell "my buffer is too small" from "the sender is broken".
///
/// [`Transceiver::Error`] is required to be constructible from this so that
/// every driver names the condition the same way rather than each inventing
/// its own spelling of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Oversized {
    /// The length of the frame that was not delivered.
    pub len: usize,
}

/// A radio, reduced to the five things an association needs from one.
///
/// Every method is fallible except by the implementor's choice of
/// [`Transceiver::Error`]: a driver whose operation genuinely cannot fail
/// returns `Ok` unconditionally, and nothing is lost, but a real radio can
/// fail a read and the interface must have somewhere to say so.
pub trait Transceiver {
    /// The driver's own failure type.
    ///
    /// Required to carry [`Oversized`] so that "the frame did not fit" is one
    /// named condition across every driver rather than one per driver.
    type Error: From<Oversized>;

    /// Put one complete 802.11 frame on the air, MAC header included and FCS
    /// excluded.
    ///
    /// # Errors
    ///
    /// Whatever the driver reports — a malformed header, a radio that is
    /// down, a full transmit queue.
    fn transmit(&mut self, frame: &[u8]) -> Result<(), Self::Error>;

    /// Copy the next received frame into `buf`.
    ///
    /// Returns `Ok(None)` when no frame is waiting, and `Ok(Some(n))` when
    /// `buf[..n]` now holds one. **"Nothing waiting" and "the read failed"
    /// are the two answers a retry loop has to tell apart**, which is why
    /// this is a `Result<Option<_>>` and not an `Option<_>`.
    ///
    /// # Errors
    ///
    /// [`Oversized`] — converted through [`Transceiver::Error`] — if the next
    /// frame is longer than `buf`; the frame must not be truncated. Otherwise
    /// whatever the driver reports.
    fn receive(&mut self, buf: &mut [u8]) -> Result<Option<usize>, Self::Error>;

    /// Install the pairwise temporal key, used for unicast traffic with the
    /// AP.
    ///
    /// Called **exactly once** per association, at the moment the handshake
    /// authorises it. A driver that is asked twice is entitled to refuse: a
    /// reinstall resets the packet number, which is the KRACK attack, and a
    /// driver that refuses is the last line of defence if this module's
    /// state machine is ever wrong.
    ///
    /// # Errors
    ///
    /// Whatever the driver reports, including a refusal to reinstall.
    fn install_pairwise_key(&mut self, key: &[u8]) -> Result<(), Self::Error>;

    /// Install a group temporal key under `key_id`, used for broadcast and
    /// multicast traffic.
    ///
    /// Unlike the pairwise key this *is* called again on a group rekey, but
    /// always under a key id the AP chose, and never twice for the same id
    /// with the same key.
    ///
    /// # Errors
    ///
    /// Whatever the driver reports.
    fn install_group_key(&mut self, key_id: u8, key: &[u8]) -> Result<(), Self::Error>;

    /// Tune the radio to `channel`, returning the channel it actually landed
    /// on.
    ///
    /// The two can differ: regulatory rules can forbid a channel the AP is
    /// legally using elsewhere, and some radios silently pick the nearest
    /// permitted one. Returning the real answer rather than `()` is what lets
    /// the caller notice, which matters because associating from the wrong
    /// channel fails in a way that looks like the AP ignoring us.
    ///
    /// # Errors
    ///
    /// Whatever the driver reports.
    fn set_channel(&mut self, channel: u8) -> Result<u8, Self::Error>;
}

// ---------------------------------------------------------------------------
// Errors, phases and steps
// ---------------------------------------------------------------------------

/// Everything that can stop an association.
///
/// Generic over the driver's error so a radio failure keeps its own detail
/// instead of being flattened into one `IoError` — "the queue was full" and
/// "the frame header was malformed" lead to different fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    /// The radio failed. Carries the driver's own error.
    Radio(E),
    /// The radio could not be tuned to the AP's channel. Associating from
    /// anywhere else cannot work, so this fails immediately rather than
    /// spending the caller's retry budget on frames nobody will hear.
    WrongChannel {
        /// The channel the AP is on.
        wanted: u8,
        /// The channel the radio landed on instead.
        got: u8,
    },
    /// The AP refused Open System authentication, with the status code it
    /// gave — see [`mgmt::status`].
    AuthRefused(u16),
    /// The AP refused the association request, with its status code.
    AssocRefused(u16),
    /// The AP deauthenticated or disassociated us, with its reason code —
    /// see [`mgmt::reason`].
    Deauthenticated(u16),
    /// The 4-way handshake rejected a message. Carries the supplicant's own
    /// fine-grained reason, so that "wrong password" ([`supplicant::Error::BadMic`])
    /// stays distinguishable from "someone edited the beacon"
    /// ([`supplicant::Error::RsnMismatch`]).
    Handshake(supplicant::Error),
    /// A frame did not fit the buffer it was being built in. Cannot happen
    /// with a [`Buffers`] and a conforming SSID and RSN element; it is
    /// reported rather than asserted because the alternative is a panic in a
    /// kernel.
    BuildFailed,
    /// [`Association::poll`] was called after the association had already
    /// failed. The original error was returned once, at the poll that
    /// produced it; this is what every later poll says.
    Aborted,
}

impl<E> From<supplicant::Error> for Error<E> {
    fn from(e: supplicant::Error) -> Self {
        Error::Handshake(e)
    }
}

/// How far the association has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing sent. The next poll tunes the radio and sends the
    /// authentication request.
    Idle,
    /// Authentication request sent; waiting for the response.
    Authenticating,
    /// Association request sent; waiting for the response.
    Associating,
    /// Associated. Waiting for the AP to start the 4-way handshake, which it
    /// does — the station never sends message 1.
    Handshaking,
    /// Keys installed on both sides; the link carries data.
    Established,
    /// Terminally failed. See [`Error::Aborted`].
    Failed,
}

/// What one call to [`Association::poll`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// No frame was waiting and there was nothing to send. The caller should
    /// wait — however it measures waiting — and poll again, giving up on its
    /// own bound.
    Idle,
    /// Something was sent, or a frame was received and consumed. Poll again
    /// immediately; there may be more.
    Progressed,
    /// The link just came up: both keys are installed. **Reported exactly
    /// once**, at the transition. Polling continues to be useful afterwards,
    /// for received data and for group rekeys.
    Established,
    /// A data frame arrived and was decapsulated.
    /// [`Buffers::ethernet`] with this length is an Ethernet II frame.
    Received {
        /// Octets of Ethernet frame available.
        len: usize,
    },
}

/// What [`Association::retransmit`] would resend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// Nothing outstanding: either nothing has been sent yet, or the last
    /// thing sent was answered.
    None,
    /// The authentication request.
    Auth,
    /// The association request.
    Assoc,
}

// ---------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------

/// The scratch space one association needs, owned by the caller.
///
/// About eight kilobytes, which is too much for a kernel stack — put it in a
/// `Box` or a `static`. It is the caller's rather than the
/// [`Association`]'s so that *where* the memory lives is the caller's
/// decision: a kernel that wants it in a DMA-reachable region, or shared
/// between the associations of several radios one at a time, can have that
/// without this module knowing anything about allocation.
///
/// The four buffers are separate fields rather than one arena because three
/// of them are live at once — a received frame is decapsulated into the
/// second while the reply is built in the third and framed in the fourth —
/// and overlapping them would be an aliasing bug that only shows up on the
/// one frame long enough to reach across.
pub struct Buffers {
    /// The received 802.11 frame, as it came off the air.
    rx: [u8; MAX_FRAME_LEN],
    /// A decapsulated Ethernet frame, inbound or outbound.
    eth: [u8; MAX_ETHERNET_LEN],
    /// An EAPOL-Key frame being built.
    eapol: [u8; MAX_EAPOL_LEN],
    /// The 802.11 frame about to be transmitted.
    tx: [u8; MAX_FRAME_LEN],
}

impl Buffers {
    /// A zeroed set of buffers.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rx: [0u8; MAX_FRAME_LEN],
            eth: [0u8; MAX_ETHERNET_LEN],
            eapol: [0u8; MAX_EAPOL_LEN],
            tx: [0u8; MAX_FRAME_LEN],
        }
    }

    /// The decapsulated Ethernet frame reported by [`Step::Received`].
    ///
    /// Returns `None` if `len` is longer than the buffer, which cannot happen
    /// for a length this module produced.
    #[must_use]
    pub fn ethernet(&self, len: usize) -> Option<&[u8]> {
        self.eth.get(..len)
    }
}

impl Default for Buffers {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for Buffers {
    /// Prints nothing but the type name. The buffers hold frames, and one of
    /// those frames is message 3 of the handshake with the GTK in it.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Buffers { .. }")
    }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// The station side of one association, from "tune the radio" to "the keys
/// are installed".
///
/// Borrows its two RSN elements from the caller, as [`supplicant::Config`]
/// does and for the same reason. The caller must own them somewhere that
/// outlives the association and is *not* the receive buffer — the AP's
/// element arrives in a beacon, and a beacon buffer is reused. Copying the
/// element out of the beacon before constructing this is the whole of the
/// ceremony.
pub struct Association<'a> {
    handshake: Handshake<'a>,
    sta: MacAddr,
    bssid: MacAddr,
    /// The station's own RSN element, kept because the association request
    /// carries it and [`Handshake`] does not lend its configuration back out.
    /// It is the *same slice* the handshake holds, not a copy: message 3 is
    /// checked against these bytes, so a second copy that could drift from
    /// them would be a second answer to a question with one answer.
    sta_rsn_element: &'a [u8],
    channel: u8,
    ssid: [u8; MAX_SSID_LEN],
    ssid_len: usize,
    rates: [u8; MAX_SUPPORTED_RATES],
    rates_len: usize,
    phase: Phase,
    /// The 802.11 sequence number of the next transmitted frame, in the
    /// twelve bits the field actually has.
    sequence: u16,
    pending: Pending,
}

impl core::fmt::Debug for Association<'_> {
    /// The phase and nothing else — same reasoning as [`Handshake`]'s: a
    /// driver logs its state machine, and a `Debug` that reaches the
    /// handshake reaches the PMK.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Association")
            .field("phase", &self.phase)
            .field("channel", &self.channel)
            .finish_non_exhaustive()
    }
}

impl<'a> Association<'a> {
    /// Prepare an association with the BSS described by `cfg`.
    ///
    /// `cfg` is the same configuration [`Handshake::new`] takes, because the
    /// handshake is the part of this that needs it: the station and AP
    /// addresses, the negotiated AKM and pairwise cipher, and the two RSN
    /// elements whose bytes message 3 is checked against. `channel` is the
    /// AP's operating channel, from the beacon's DS Parameter Set.
    ///
    /// `snonce` must be freshly random for every association. Reusing one
    /// across two associations with the same AP and the same PMK derives the
    /// same PTK twice, which is a nonce reuse in every frame that follows.
    ///
    /// Returns `None` if the SSID or rates are longer than the elements that
    /// carry them, or if the AKM and cipher in `cfg` are not ones this stack
    /// implements.
    #[must_use]
    pub fn new(
        cfg: Config<'a>,
        ssid: &[u8],
        rates: &[u8],
        channel: u8,
        pmk: &[u8; PMK_LEN],
        snonce: [u8; NONCE_LEN],
    ) -> Option<Self> {
        if ssid.len() > MAX_SSID_LEN || rates.len() > MAX_SUPPORTED_RATES {
            return None;
        }
        let sta = cfg.sta;
        let bssid = cfg.bssid;
        let sta_rsn_element = cfg.sta_rsn_element;
        let handshake = Handshake::new(cfg, pmk, snonce)?;

        let mut ssid_buf = [0u8; MAX_SSID_LEN];
        ssid_buf.get_mut(..ssid.len())?.copy_from_slice(ssid);
        let mut rates_buf = [0u8; MAX_SUPPORTED_RATES];
        rates_buf.get_mut(..rates.len())?.copy_from_slice(rates);

        Some(Self {
            handshake,
            sta,
            bssid,
            sta_rsn_element,
            channel,
            ssid: ssid_buf,
            ssid_len: ssid.len(),
            rates: rates_buf,
            rates_len: rates.len(),
            phase: Phase::Idle,
            sequence: 0,
            pending: Pending::None,
        })
    }

    /// How far the association has got.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// True once both keys are installed.
    ///
    /// This says the two ends derived the same PTK and the handshake
    /// completed. It does **not** say traffic is encrypted — that depends on
    /// the driver or the hardware actually applying the key it was given.
    #[must_use]
    pub fn is_established(&self) -> bool {
        self.phase == Phase::Established
    }

    /// The pairwise temporal key, once installed.
    ///
    /// Exposed for tests and for a driver that must re-arm hardware after a
    /// reset. Empty before the handshake completes.
    #[must_use]
    pub fn tk(&self) -> &[u8] {
        self.handshake.tk()
    }

    /// The group temporal key and its key id, once known.
    #[must_use]
    pub fn gtk(&self) -> Option<(u8, &[u8])> {
        self.handshake.gtk()
    }

    /// Do at most one thing, and say what it was.
    ///
    /// Call this in a loop the caller bounds. It never blocks and never
    /// sleeps; [`Step::Idle`] is how it says "nothing has arrived yet, wait
    /// however you wait".
    ///
    /// # Errors
    ///
    /// See [`Error`]. Any error is terminal: the association moves to
    /// [`Phase::Failed`] and every later poll returns [`Error::Aborted`].
    /// A frame that is merely *not for us* — a beacon from a neighbouring
    /// BSS, a data frame for another station — is discarded and reported as
    /// [`Step::Progressed`], not as an error, because it is the expected case
    /// on a shared medium.
    pub fn poll<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
    ) -> Result<Step, Error<D::Error>> {
        match self.step(dev, bufs) {
            Err(e) => {
                self.phase = Phase::Failed;
                Err(e)
            }
            ok => ok,
        }
    }

    /// The body of [`Association::poll`], without the failure latch.
    fn step<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
    ) -> Result<Step, Error<D::Error>> {
        if self.phase == Phase::Failed {
            return Err(Error::Aborted);
        }
        if self.phase == Phase::Idle {
            self.tune(dev)?;
            self.send_auth(dev, bufs)?;
            self.phase = Phase::Authenticating;
            self.pending = Pending::Auth;
            return Ok(Step::Progressed);
        }

        let Some(n) = dev.receive(&mut bufs.rx).map_err(Error::Radio)? else {
            return Ok(Step::Idle);
        };
        self.on_frame(dev, bufs, n)
    }

    /// Resend whichever request is outstanding, if any.
    ///
    /// Returns `true` if a frame went out. There is nothing to resend once
    /// the AP has answered the association request: from that point on the AP
    /// drives, and the handshake's own retransmissions are the AP's to make.
    ///
    /// This is separate from [`Association::poll`] because deciding *when* a
    /// request has gone unanswered long enough is a timing decision, and this
    /// module has no clock. The caller's loop, which does, calls this when
    /// its own timer expires.
    ///
    /// # Errors
    ///
    /// See [`Error`]. As with `poll`, an error is terminal.
    pub fn retransmit<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
    ) -> Result<bool, Error<D::Error>> {
        let result = match self.pending {
            Pending::None => return Ok(false),
            Pending::Auth => self.send_auth(dev, bufs),
            Pending::Assoc => self.send_assoc(dev, bufs),
        };
        match result {
            Ok(()) => Ok(true),
            Err(e) => {
                self.phase = Phase::Failed;
                Err(e)
            }
        }
    }

    /// Send one Ethernet II frame over the established link.
    ///
    /// The Protected Frame bit is set, which asserts the body has been
    /// encrypted; encrypting it is the driver's or the hardware's job with
    /// the key [`Transceiver::install_pairwise_key`] was given.
    ///
    /// # Errors
    ///
    /// [`Error::Aborted`] if the link is not established — sending data
    /// before the keys are in place would go out in the clear under a bit
    /// claiming otherwise, and the AP would discard it rather than misread
    /// it, so this refuses instead of silently doing nothing useful.
    /// Otherwise see [`Error`].
    pub fn send<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
        ethernet: &[u8],
    ) -> Result<(), Error<D::Error>> {
        if self.phase != Phase::Established {
            return Err(Error::Aborted);
        }
        let seq = self.next_sequence();
        let len =
            supplicant::encapsulate(&mut bufs.tx, &self.sta, &self.bssid, ethernet, seq, true)
                .ok_or(Error::BuildFailed)?;
        let frame = bufs.tx.get(..len).ok_or(Error::BuildFailed)?;
        dev.transmit(frame).map_err(Error::Radio)
    }

    // -- the pieces ---------------------------------------------------------

    /// Tune the radio, and refuse to continue if it landed elsewhere.
    fn tune<D: Transceiver>(&self, dev: &mut D) -> Result<(), Error<D::Error>> {
        let got = dev.set_channel(self.channel).map_err(Error::Radio)?;
        if got == self.channel {
            Ok(())
        } else {
            Err(Error::WrongChannel {
                wanted: self.channel,
                got,
            })
        }
    }

    fn send_auth<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
    ) -> Result<(), Error<D::Error>> {
        let seq = self.next_sequence();
        let len = supplicant::write_auth_request(&mut bufs.tx, &self.sta, &self.bssid, seq)
            .ok_or(Error::BuildFailed)?;
        let frame = bufs.tx.get(..len).ok_or(Error::BuildFailed)?;
        dev.transmit(frame).map_err(Error::Radio)
    }

    fn send_assoc<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
    ) -> Result<(), Error<D::Error>> {
        let seq = self.next_sequence();
        let ssid = self.ssid.get(..self.ssid_len).ok_or(Error::BuildFailed)?;
        let rates = self.rates.get(..self.rates_len).ok_or(Error::BuildFailed)?;
        let len = supplicant::write_assoc_request(
            &mut bufs.tx,
            &self.sta,
            &self.bssid,
            ssid,
            rates,
            self.sta_rsn_element,
            seq,
        )
        .ok_or(Error::BuildFailed)?;
        let frame = bufs.tx.get(..len).ok_or(Error::BuildFailed)?;
        dev.transmit(frame).map_err(Error::Radio)
    }

    /// The sequence number for the next frame, in the twelve bits the field
    /// has. Wraps, as the standard requires.
    fn next_sequence(&mut self) -> u16 {
        let seq = self.sequence;
        self.sequence = self.sequence.wrapping_add(1) & 0x0FFF;
        seq
    }

    /// Dispatch one received frame.
    fn on_frame<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
        n: usize,
    ) -> Result<Step, Error<D::Error>> {
        // Everything below re-reads `bufs.rx[..n]`; take the length once so a
        // later `get` cannot silently look past what was received.
        let Some(header) = frame::MacHeader::parse(bufs.rx.get(..n).ok_or(Error::BuildFailed)?)
        else {
            // Not a parseable header. On a shared medium that is noise, not a
            // failure of this association.
            return Ok(Step::Progressed);
        };
        match header.fc.frame_type() {
            FrameType::Management => self.on_management(dev, bufs, n, &header),
            FrameType::Data => self.on_data(dev, bufs, n, &header),
            // Control frames are the MAC's business and never carry anything
            // this state machine acts on.
            _ => Ok(Step::Progressed),
        }
    }

    fn on_management<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
        n: usize,
        header: &frame::MacHeader,
    ) -> Result<Step, Error<D::Error>> {
        // Addressed to us, from our AP. Without both checks a neighbouring
        // BSS's association response would advance our state machine.
        if header.addr1 != self.sta || header.addr2 != Some(self.bssid) {
            return Ok(Step::Progressed);
        }
        let raw = bufs.rx.get(..n).ok_or(Error::BuildFailed)?;
        let Some(parsed) = frame::Frame::parse(raw) else {
            return Ok(Step::Progressed);
        };
        let Some(body) = mgmt::Body::parse(header.fc.subtype(), parsed.body) else {
            return Ok(Step::Progressed);
        };

        match body {
            // A deauthentication or disassociation ends the association
            // whatever phase it is in. There is no recovery inside this
            // object: the AP has discarded its side of the state, so
            // continuing would mean talking to a peer that has forgotten us.
            mgmt::Body::Deauth(d) | mgmt::Body::Disassoc(d) => {
                Err(Error::Deauthenticated(d.reason))
            }
            mgmt::Body::Auth(a) if self.phase == Phase::Authenticating => {
                // Only the algorithm we asked for, and only the second frame
                // of the exchange: an Open System response with sequence 1 is
                // our own request heard back, and Shared Key would mean the
                // AP answered a question we did not ask.
                if a.algorithm != mgmt::auth_alg::OPEN_SYSTEM || a.seq != 2 {
                    return Ok(Step::Progressed);
                }
                if a.status != mgmt::status::SUCCESS {
                    return Err(Error::AuthRefused(a.status));
                }
                self.send_assoc(dev, bufs)?;
                self.phase = Phase::Associating;
                self.pending = Pending::Assoc;
                Ok(Step::Progressed)
            }
            mgmt::Body::AssocResp(r) | mgmt::Body::ReassocResp(r)
                if self.phase == Phase::Associating =>
            {
                if !r.accepted() {
                    return Err(Error::AssocRefused(r.status));
                }
                // Associated. The AP now starts the 4-way handshake; a
                // station never sends message 1, so there is nothing to
                // transmit here and nothing outstanding to retransmit.
                self.phase = Phase::Handshaking;
                self.pending = Pending::None;
                Ok(Step::Progressed)
            }
            _ => Ok(Step::Progressed),
        }
    }

    fn on_data<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
        n: usize,
        header: &frame::MacHeader,
    ) -> Result<Step, Error<D::Error>> {
        let Some(roles) = header.data_addr_roles() else {
            return Ok(Step::Progressed);
        };
        if roles.receiver != self.sta || roles.bssid != Some(self.bssid) {
            return Ok(Step::Progressed);
        }
        let raw = bufs.rx.get(..n).ok_or(Error::BuildFailed)?;
        let Some(len) = supplicant::decapsulate(&mut bufs.eth, raw) else {
            return Ok(Step::Progressed);
        };
        let Some(eth) = bufs.eth.get(..len) else {
            return Ok(Step::Progressed);
        };
        let (Some(hi), Some(lo)) = (eth.get(12), eth.get(13)) else {
            return Ok(Step::Progressed);
        };
        let ethertype = u16::from_be_bytes([*hi, *lo]);
        if ethertype != llc::ETHERTYPE_EAPOL {
            return Ok(Step::Received { len });
        }
        self.on_eapol(dev, bufs, len)
    }

    /// Feed one EAPOL frame to the handshake and act on what it says.
    ///
    /// `len` is the length of the Ethernet frame in `bufs.eth`; the EAPOL
    /// frame is its payload.
    fn on_eapol<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
        len: usize,
    ) -> Result<Step, Error<D::Error>> {
        // Borrowing `bufs.eth` and `bufs.eapol` at once is why they are
        // separate fields: the frame being read and the reply being written
        // are live simultaneously.
        let payload = bufs
            .eth
            .get(llc::ETHERNET_HEADER_LEN..len)
            .ok_or(Error::BuildFailed)?;
        let outcome = self.handshake.on_eapol(payload, &mut bufs.eapol)?;

        if !outcome.is_empty() {
            self.send_eapol(dev, bufs, outcome.len())?;
        }

        if !outcome.installs_keys() {
            // `Outcome::Retransmission` lands here, and that is the whole
            // point of the variant: the AP did not hear message 4, we send it
            // again, and we do **not** reinstall the key. Reinstalling resets
            // the packet number, which is KRACK.
            return Ok(Step::Progressed);
        }

        if self.phase == Phase::Established {
            // A group rekey: only the group key moves, and the pairwise key
            // must not be touched.
            self.install_group(dev)?;
            return Ok(Step::Progressed);
        }

        let tk = self.handshake.tk();
        dev.install_pairwise_key(tk).map_err(Error::Radio)?;
        self.install_group(dev)?;
        self.phase = Phase::Established;
        self.pending = Pending::None;
        Ok(Step::Established)
    }

    fn install_group<D: Transceiver>(&self, dev: &mut D) -> Result<(), Error<D::Error>> {
        // A handshake that authorised installation without a GTK would be a
        // bug in `supplicant`, which refuses message 3 with no GTK KDE. There
        // is nothing useful to do about it here, and treating it as fatal
        // would turn that bug into an unassociable radio, so the pairwise key
        // stands on its own.
        if let Some((id, gtk)) = self.handshake.gtk() {
            dev.install_group_key(id, gtk).map_err(Error::Radio)?;
        }
        Ok(())
    }

    /// Frame an EAPOL reply as an Ethernet frame, encapsulate it, transmit.
    fn send_eapol<D: Transceiver>(
        &mut self,
        dev: &mut D,
        bufs: &mut Buffers,
        len: usize,
    ) -> Result<(), Error<D::Error>> {
        let total = llc::ETHERNET_HEADER_LEN
            .checked_add(len)
            .ok_or(Error::BuildFailed)?;
        {
            let eth = bufs.eth.get_mut(..total).ok_or(Error::BuildFailed)?;
            eth.get_mut(..6)
                .ok_or(Error::BuildFailed)?
                .copy_from_slice(&self.bssid);
            eth.get_mut(6..12)
                .ok_or(Error::BuildFailed)?
                .copy_from_slice(&self.sta);
            eth.get_mut(12..14)
                .ok_or(Error::BuildFailed)?
                .copy_from_slice(&llc::ETHERTYPE_EAPOL.to_be_bytes());
        }
        let body = bufs.eapol.get(..len).ok_or(Error::BuildFailed)?;
        bufs.eth
            .get_mut(llc::ETHERNET_HEADER_LEN..total)
            .ok_or(Error::BuildFailed)?
            .copy_from_slice(body);

        // Message 2 and message 4 go out unprotected — the keys are not
        // installed on either side yet. Group message 2, sent after the link
        // is up, is protected. Getting this backwards sends plaintext under a
        // bit claiming otherwise, which the AP discards.
        let protected = self.phase == Phase::Established;
        let seq = self.next_sequence();
        let ethernet = bufs.eth.get(..total).ok_or(Error::BuildFailed)?;
        let n = supplicant::encapsulate(
            &mut bufs.tx,
            &self.sta,
            &self.bssid,
            ethernet,
            seq,
            protected,
        )
        .ok_or(Error::BuildFailed)?;
        let frame = bufs.tx.get(..n).ok_or(Error::BuildFailed)?;
        dev.transmit(frame).map_err(Error::Radio)
    }
}

// The five defensive lints the workspace turns on are for production code: a
// test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and an assertion that fails by panicking is a
// test doing its job rather than a robustness hole.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::eapol::{self, key_info};
    use crate::frame::{FrameControl, MacHeader, SeqCtrl, mgmt_subtype};
    use crate::ie;
    use crate::kdf::{self, Kdf, MicAlgo, Ptk};
    use crate::rsn;
    use std::collections::VecDeque;

    const AP: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    const STA: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x02];
    const OTHER: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x09];
    const SSID: &[u8] = b"slateos-test";
    const CHANNEL: u8 = 6;
    const ANONCE: [u8; NONCE_LEN] = [0xA0; NONCE_LEN];
    const SNONCE: [u8; NONCE_LEN] = [0x50; NONCE_LEN];
    const PMK: [u8; PMK_LEN] = [0x0B; PMK_LEN];
    const GTK: [u8; 16] = [0x67; 16];
    const GTK2: [u8; 16] = [0x77; 16];

    /// The RSN element body a WPA2-PSK/CCMP AP advertises.
    const RSN_ELEMENT: [u8; 20] = [
        0x01, 0x00, // version 1
        0x00, 0x0F, 0xAC, 0x04, // group cipher CCMP
        0x01, 0x00, 0x00, 0x0F, 0xAC, 0x04, // one pairwise: CCMP
        0x01, 0x00, 0x00, 0x0F, 0xAC, 0x02, // one AKM: PSK
        0x00, 0x00, // capabilities
    ];

    // -- the mock radio -----------------------------------------------------

    /// A driver error type with exactly the two failures a test needs.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum MockError {
        /// The frame did not fit the buffer offered.
        Oversized(usize),
        /// The radio refused to install a key twice — the backstop a real
        /// driver provides against KRACK.
        KeyReinstall,
    }

    impl From<Oversized> for MockError {
        fn from(o: Oversized) -> Self {
            MockError::Oversized(o.len)
        }
    }

    /// A radio that hands out queued frames, records what was transmitted,
    /// and refuses to install the same key class twice.
    struct MockRadio {
        rx: VecDeque<Vec<u8>>,
        tx: Vec<Vec<u8>>,
        pairwise: Vec<Vec<u8>>,
        group: Vec<(u8, Vec<u8>)>,
        /// Refusals counted rather than returned, so a test can assert the
        /// refusal happened *and* that the driver never provoked it.
        reinstalls_refused: usize,
        channel: u8,
        /// What `set_channel` reports landing on, if not what was asked.
        forced_channel: Option<u8>,
    }

    impl MockRadio {
        fn new() -> Self {
            Self {
                rx: VecDeque::new(),
                tx: Vec::new(),
                pairwise: Vec::new(),
                group: Vec::new(),
                reinstalls_refused: 0,
                channel: 0,
                forced_channel: None,
            }
        }

        fn queue(&mut self, frame: &[u8]) {
            self.rx.push_back(frame.to_vec());
        }
    }

    impl Transceiver for MockRadio {
        type Error = MockError;

        fn transmit(&mut self, frame: &[u8]) -> Result<(), MockError> {
            self.tx.push(frame.to_vec());
            Ok(())
        }

        fn receive(&mut self, buf: &mut [u8]) -> Result<Option<usize>, MockError> {
            let Some(front) = self.rx.front() else {
                return Ok(None);
            };
            if front.len() > buf.len() {
                // Deliberately does not pop: the frame is still waiting, and
                // a larger buffer would get it.
                return Err(Oversized { len: front.len() }.into());
            }
            let frame = self.rx.pop_front().expect("just checked");
            buf[..frame.len()].copy_from_slice(&frame);
            Ok(Some(frame.len()))
        }

        fn install_pairwise_key(&mut self, key: &[u8]) -> Result<(), MockError> {
            if !self.pairwise.is_empty() {
                self.reinstalls_refused += 1;
                return Err(MockError::KeyReinstall);
            }
            self.pairwise.push(key.to_vec());
            Ok(())
        }

        fn install_group_key(&mut self, key_id: u8, key: &[u8]) -> Result<(), MockError> {
            self.group.push((key_id, key.to_vec()));
            Ok(())
        }

        fn set_channel(&mut self, channel: u8) -> Result<u8, MockError> {
            self.channel = self.forced_channel.unwrap_or(channel);
            Ok(self.channel)
        }
    }

    // -- the mock AP --------------------------------------------------------

    fn config() -> Config<'static> {
        Config {
            sta: STA,
            bssid: AP,
            akm: rsn::akm::PSK,
            pairwise: rsn::Suite::standard(rsn::cipher::CCMP_128),
            sta_rsn_element: &RSN_ELEMENT,
            ap_rsn_element: &RSN_ELEMENT,
        }
    }

    fn association() -> Association<'static> {
        Association::new(config(), SSID, &BASIC_RATES, CHANNEL, &PMK, SNONCE)
            .expect("WPA2-PSK/CCMP with a short SSID is supported")
    }

    fn ptk() -> Ptk {
        kdf::derive_ptk(Kdf::Sha1, &PMK, &AP, &STA, &ANONCE, &SNONCE, 16)
            .expect("a 16-octet TK is in range")
    }

    /// A management frame from the AP to `dst`, body already built.
    fn ap_mgmt(subtype: u8, dst: MacAddr, bssid: MacAddr, body: &[u8]) -> Vec<u8> {
        let header = MacHeader {
            fc: FrameControl::new(FrameType::Management, subtype),
            duration_id: 0,
            addr1: dst,
            addr2: Some(bssid),
            addr3: Some(bssid),
            seq_ctrl: Some(SeqCtrl::new(0, 0)),
            addr4: None,
            qos_ctrl: None,
            ht_ctrl: None,
            header_len: 24,
        };
        let mut out = [0u8; 512];
        let mut off = header.write(&mut out).expect("header fits");
        out[off..off + body.len()].copy_from_slice(body);
        off += body.len();
        out[..off].to_vec()
    }

    fn auth_response(status: u16) -> Vec<u8> {
        let mut body = [0u8; 16];
        let mut off = 0usize;
        mgmt::write_auth(&mut body, &mut off, mgmt::auth_alg::OPEN_SYSTEM, 2, status)
            .expect("fits");
        ap_mgmt(mgmt_subtype::AUTH, STA, AP, &body[..off])
    }

    fn assoc_response(status: u16) -> Vec<u8> {
        let mut body = [0u8; 64];
        // Capability, status, AID; then the AP's rates. There is no
        // `write_assoc_resp` in `mgmt` because nothing in this stack is an
        // AP, so the six fixed octets are laid out here.
        body[0..2].copy_from_slice(&mgmt::capability::ESS.to_le_bytes());
        body[2..4].copy_from_slice(&status.to_le_bytes());
        body[4..6].copy_from_slice(&0xC001u16.to_le_bytes());
        let mut off = 6usize;
        ie::write_element(&mut body, &mut off, ie::id::SUPPORTED_RATES, &BASIC_RATES)
            .expect("fits");
        ap_mgmt(mgmt_subtype::ASSOC_RESP, STA, AP, &body[..off])
    }

    fn deauth(reason: u16) -> Vec<u8> {
        let mut body = [0u8; 8];
        let mut off = 0usize;
        mgmt::write_deauth(&mut body, &mut off, reason).expect("fits");
        ap_mgmt(mgmt_subtype::DEAUTH, STA, AP, &body[..off])
    }

    /// Wrap a payload as a from-DS 802.11 data frame, AP to station.
    fn ap_data(
        dst: MacAddr,
        bssid: MacAddr,
        src: MacAddr,
        ethertype: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let header = MacHeader {
            fc: FrameControl::new(FrameType::Data, frame::data_subtype::DATA).with_from_ds(true),
            duration_id: 0,
            // From the DS: address 1 is the station, 2 the AP, 3 the original
            // source.
            addr1: dst,
            addr2: Some(bssid),
            addr3: Some(src),
            seq_ctrl: Some(SeqCtrl::new(0, 0)),
            addr4: None,
            qos_ctrl: None,
            ht_ctrl: None,
            header_len: 24,
        };
        let mut out = [0u8; MAX_FRAME_LEN];
        let mut off = header.write(&mut out).expect("header fits");
        let n = llc::write_header(&mut out[off..], ethertype).expect("SNAP fits");
        off += n;
        out[off..off + payload.len()].copy_from_slice(payload);
        off += payload.len();
        out[..off].to_vec()
    }

    fn ap_eapol(payload: &[u8]) -> Vec<u8> {
        ap_data(STA, AP, AP, llc::ETHERTYPE_EAPOL, payload)
    }

    /// Build one authenticator-to-station EAPOL-Key frame.
    fn ap_key_frame(
        flags: u16,
        replay: u64,
        nonce: [u8; NONCE_LEN],
        key_data: &[u8],
        encrypt: bool,
    ) -> Vec<u8> {
        let ptk = ptk();
        let mut wrapped = [0u8; 256];
        let payload: &[u8] = if encrypt {
            let cipher = aes::Aes::new(&ptk.kek).expect("16-octet KEK");
            let n = aes::keywrap::wrap(&cipher, &mut wrapped, key_data).expect("wraps");
            &wrapped[..n]
        } else {
            key_data
        };

        let mut info = 2u16 | flags; // descriptor version 2: HMAC-SHA1 + key wrap
        if encrypt {
            info |= key_info::ENCRYPTED_KEY_DATA;
        }
        let fields = eapol::KeyFrameFields {
            descriptor_type: eapol::descriptor_type::RSN,
            key_info: info,
            key_len: 16,
            replay_counter: replay,
            nonce,
            iv: [0u8; eapol::IV_LEN],
            rsc: [0u8; eapol::RSC_LEN],
            key_data: payload,
        };
        let mut out = [0u8; 512];
        let n = eapol::write(
            &mut out,
            eapol::version::V2,
            &fields,
            eapol::MIC_LEN_DEFAULT,
        )
        .expect("fits in 512");
        if (info & key_info::KEY_MIC) != 0 {
            let mut mic = [0u8; eapol::MIC_LEN_DEFAULT];
            kdf::compute_mic(
                MicAlgo::HmacSha1,
                &ptk.kck,
                &out[..n],
                eapol::MIC_LEN_DEFAULT,
                &mut mic,
            )
            .expect("MIC computes");
            eapol::set_mic(&mut out[..n], &mic).expect("MIC fits");
        }
        out[..n].to_vec()
    }

    fn gtk_kde(buf: &mut [u8], off: &mut usize, gtk: &[u8], key_id: u8) {
        buf[*off] = 221; // vendor-specific
        buf[*off + 1] = (4 + 2 + gtk.len()) as u8;
        buf[*off + 2..*off + 5].copy_from_slice(&rsn::IEEE_OUI);
        buf[*off + 5] = kdf::kde::GTK;
        buf[*off + 6] = key_id & 0x03;
        buf[*off + 7] = 0;
        buf[*off + 8..*off + 8 + gtk.len()].copy_from_slice(gtk);
        *off += 8 + gtk.len();
        while !off.is_multiple_of(8) {
            buf[*off] = 0;
            *off += 1;
        }
    }

    fn m1(replay: u64) -> Vec<u8> {
        ap_key_frame(
            key_info::PAIRWISE | key_info::KEY_ACK,
            replay,
            ANONCE,
            &[],
            false,
        )
    }

    fn m3(replay: u64, gtk: &[u8], key_id: u8) -> Vec<u8> {
        let mut buf = [0u8; 128];
        let mut off = 0usize;
        buf[off] = ie::id::RSN;
        buf[off + 1] = RSN_ELEMENT.len() as u8;
        buf[off + 2..off + 2 + RSN_ELEMENT.len()].copy_from_slice(&RSN_ELEMENT);
        off += 2 + RSN_ELEMENT.len();
        gtk_kde(&mut buf, &mut off, gtk, key_id);
        ap_key_frame(
            key_info::PAIRWISE | key_info::KEY_ACK | key_info::KEY_MIC | key_info::SECURE,
            replay,
            ANONCE,
            &buf[..off],
            true,
        )
    }

    fn group_m1(replay: u64, gtk: &[u8], key_id: u8) -> Vec<u8> {
        let mut buf = [0u8; 128];
        let mut off = 0usize;
        gtk_kde(&mut buf, &mut off, gtk, key_id);
        ap_key_frame(
            key_info::KEY_ACK | key_info::KEY_MIC | key_info::SECURE,
            replay,
            [0u8; NONCE_LEN],
            &buf[..off],
            true,
        )
    }

    /// Poll until the radio's queue is drained, collecting every step.
    fn drain(
        a: &mut Association<'_>,
        dev: &mut MockRadio,
        bufs: &mut Buffers,
    ) -> Result<Vec<Step>, Error<MockError>> {
        let mut steps = Vec::new();
        loop {
            let step = a.poll(dev, bufs)?;
            steps.push(step);
            if step == Step::Idle {
                return Ok(steps);
            }
        }
    }

    /// Run a complete association and hand back the pieces to assert on.
    fn associate() -> (Association<'static>, MockRadio, Box<Buffers>) {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Box::new(Buffers::new());

        dev.queue(&auth_response(mgmt::status::SUCCESS));
        dev.queue(&assoc_response(mgmt::status::SUCCESS));
        dev.queue(&ap_eapol(&m1(1)));
        dev.queue(&ap_eapol(&m3(2, &GTK, 1)));

        let steps = drain(&mut a, &mut dev, &mut bufs).expect("the association succeeds");
        assert!(
            steps.contains(&Step::Established),
            "the link comes up exactly once: {steps:?}"
        );
        (a, dev, bufs)
    }

    // -- the happy path -----------------------------------------------------

    #[test]
    fn a_whole_association_tunes_authenticates_associates_and_installs_both_keys() {
        let (a, dev, _) = associate();
        assert_eq!(a.phase(), Phase::Established);
        assert!(a.is_established());
        assert_eq!(dev.channel, CHANNEL);
        assert_eq!(dev.pairwise.len(), 1, "the pairwise key is installed once");
        assert_eq!(dev.pairwise[0], ptk().tk());
        assert_eq!(dev.group.len(), 1);
        assert_eq!(dev.group[0], (1, GTK.to_vec()));
        assert_eq!(dev.reinstalls_refused, 0, "the driver was never provoked");
    }

    #[test]
    fn the_link_comes_up_exactly_once() {
        let (mut a, mut dev, mut bufs) = associate();
        // Nothing more to receive; polling again must not re-announce.
        assert_eq!(a.poll(&mut dev, &mut bufs).expect("idle"), Step::Idle);
        assert_eq!(a.poll(&mut dev, &mut bufs).expect("idle"), Step::Idle);
    }

    #[test]
    fn four_frames_go_out_in_the_right_order() {
        let (_, dev, _) = associate();
        assert_eq!(dev.tx.len(), 4, "auth, assoc, m2, m4");

        let subtype = |f: &[u8]| MacHeader::parse(f).expect("parses").fc.subtype();
        let kind = |f: &[u8]| MacHeader::parse(f).expect("parses").fc.frame_type();
        assert_eq!(kind(&dev.tx[0]), FrameType::Management);
        assert_eq!(subtype(&dev.tx[0]), mgmt_subtype::AUTH);
        assert_eq!(subtype(&dev.tx[1]), mgmt_subtype::ASSOC_REQ);
        assert_eq!(kind(&dev.tx[2]), FrameType::Data);
        assert_eq!(kind(&dev.tx[3]), FrameType::Data);
    }

    #[test]
    fn the_association_request_carries_our_ssid_rates_and_rsn_element() {
        let (_, dev, _) = associate();
        let parsed = frame::Frame::parse(&dev.tx[1]).expect("parses");
        let body = mgmt::Body::parse(mgmt_subtype::ASSOC_REQ, parsed.body).expect("parses");
        let elements = body.elements();
        assert_eq!(ie::ssid(elements), Some(SSID));
        assert_eq!(
            ie::Elements::find_id(elements, ie::id::SUPPORTED_RATES).map(|e| e.data),
            Some(&BASIC_RATES[..])
        );
        assert_eq!(
            ie::Elements::find_id(elements, ie::id::RSN).map(|e| e.data),
            Some(&RSN_ELEMENT[..]),
            "the element message 3 will be checked against is the one we sent"
        );
    }

    #[test]
    fn the_handshake_replies_go_out_unprotected_and_data_goes_out_protected() {
        let (mut a, mut dev, mut bufs) = associate();
        for (i, frame) in dev.tx.iter().enumerate().skip(2) {
            let h = MacHeader::parse(frame).expect("parses");
            assert!(
                !h.fc.protected(),
                "frame {i} is a handshake reply sent before the keys are in place"
            );
        }
        let eth = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // dst
            0x02, 0x00, 0x00, 0x00, 0x00, 0x02, // src
            0x08, 0x00, // IPv4
            0xDE, 0xAD,
        ];
        a.send(&mut dev, &mut bufs, &eth).expect("sends");
        let last = dev.tx.last().expect("a frame");
        let h = MacHeader::parse(last).expect("parses");
        assert!(
            h.fc.protected(),
            "data over an established link is protected"
        );
        assert!(h.fc.to_ds(), "a station sends to the distribution system");
    }

    // -- the KRACK backstop -------------------------------------------------

    #[test]
    fn a_replayed_message_three_replies_but_does_not_reinstall_the_key() {
        let (mut a, mut dev, mut bufs) = associate();
        let before = dev.tx.len();

        // The AP did not hear message 4 and asks again, with the next replay
        // counter as a real AP would.
        dev.queue(&ap_eapol(&m3(3, &GTK, 1)));
        let steps = drain(&mut a, &mut dev, &mut bufs).expect("a retransmission is accepted");

        assert!(
            !steps.contains(&Step::Established),
            "the link does not come up a second time"
        );
        assert_eq!(
            dev.tx.len(),
            before + 1,
            "message 4 is sent again — the AP is waiting for it"
        );
        assert_eq!(
            dev.pairwise.len(),
            1,
            "the pairwise key is installed exactly once across a replay"
        );
        assert_eq!(
            dev.reinstalls_refused, 0,
            "the driver's refusal was never reached: this module did not ask"
        );
        assert_eq!(dev.group.len(), 1, "and the group key is not reinstalled");
        assert!(a.is_established());
    }

    #[test]
    fn a_group_rekey_installs_only_the_group_key() {
        let (mut a, mut dev, mut bufs) = associate();
        dev.queue(&ap_eapol(&group_m1(4, &GTK2, 2)));
        drain(&mut a, &mut dev, &mut bufs).expect("a group rekey is accepted");

        assert_eq!(dev.pairwise.len(), 1, "the pairwise key is untouched");
        assert_eq!(dev.group.len(), 2);
        assert_eq!(dev.group[1], (2, GTK2.to_vec()));
        assert_eq!(dev.reinstalls_refused, 0);
    }

    #[test]
    fn a_replayed_group_message_is_refused_rather_than_reinstalled() {
        let (mut a, mut dev, mut bufs) = associate();
        dev.queue(&ap_eapol(&group_m1(4, &GTK2, 2)));
        drain(&mut a, &mut dev, &mut bufs).expect("the rekey is accepted");

        // The same frame again: the replay counter has not advanced, so the
        // supplicant refuses it and no key moves.
        dev.queue(&ap_eapol(&group_m1(4, &GTK2, 2)));
        let err = drain(&mut a, &mut dev, &mut bufs).expect_err("a replay is refused");
        assert_eq!(err, Error::Handshake(supplicant::Error::Replay));
        assert_eq!(dev.group.len(), 2, "no third install");
    }

    // -- refusals and interruptions -----------------------------------------

    #[test]
    fn a_refused_authentication_reports_the_status_code() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();
        dev.queue(&auth_response(mgmt::status::UNSUPPORTED_AUTH_ALGORITHM));
        let err = drain(&mut a, &mut dev, &mut bufs).expect_err("refused");
        assert_eq!(
            err,
            Error::AuthRefused(mgmt::status::UNSUPPORTED_AUTH_ALGORITHM)
        );
        assert_eq!(a.phase(), Phase::Failed);
    }

    #[test]
    fn a_refused_association_reports_the_status_code() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();
        dev.queue(&auth_response(mgmt::status::SUCCESS));
        dev.queue(&assoc_response(mgmt::status::AP_FULL));
        let err = drain(&mut a, &mut dev, &mut bufs).expect_err("refused");
        assert_eq!(err, Error::AssocRefused(mgmt::status::AP_FULL));
    }

    #[test]
    fn a_deauthentication_ends_the_association_in_any_phase() {
        let (mut a, mut dev, mut bufs) = associate();
        dev.queue(&deauth(mgmt::reason::INACTIVITY));
        let err = drain(&mut a, &mut dev, &mut bufs).expect_err("deauthenticated");
        assert_eq!(err, Error::Deauthenticated(mgmt::reason::INACTIVITY));
        assert!(!a.is_established());
    }

    #[test]
    fn every_poll_after_a_failure_says_aborted() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();
        dev.queue(&auth_response(mgmt::status::AP_FULL));
        drain(&mut a, &mut dev, &mut bufs).expect_err("refused");
        assert_eq!(
            a.poll(&mut dev, &mut bufs).expect_err("stays failed"),
            Error::Aborted
        );
        assert_eq!(
            a.poll(&mut dev, &mut bufs).expect_err("stays failed"),
            Error::Aborted
        );
    }

    #[test]
    fn sending_data_before_the_link_is_up_is_refused_rather_than_sent_in_the_clear() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();
        let eth = [0u8; 20];
        assert_eq!(
            a.send(&mut dev, &mut bufs, &eth).expect_err("refused"),
            Error::Aborted
        );
        assert!(dev.tx.is_empty());
    }

    // -- the channel --------------------------------------------------------

    #[test]
    fn a_radio_that_lands_on_another_channel_fails_immediately() {
        let mut a = association();
        let mut dev = MockRadio::new();
        dev.forced_channel = Some(11);
        let mut bufs = Buffers::new();
        let err = a.poll(&mut dev, &mut bufs).expect_err("wrong channel");
        assert_eq!(
            err,
            Error::WrongChannel {
                wanted: CHANNEL,
                got: 11
            }
        );
        assert!(
            dev.tx.is_empty(),
            "no request is spent on a channel nobody is listening to"
        );
    }

    // -- frames that are not ours -------------------------------------------

    #[test]
    fn a_management_frame_from_another_bss_does_not_advance_the_state_machine() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();

        // An association response from a different AP, and one addressed to a
        // different station. Neither may be believed.
        let mut body = [0u8; 8];
        body[0..2].copy_from_slice(&mgmt::capability::ESS.to_le_bytes());
        body[4..6].copy_from_slice(&0xC001u16.to_le_bytes());
        dev.queue(&auth_response(mgmt::status::SUCCESS));
        dev.queue(&ap_mgmt(mgmt_subtype::ASSOC_RESP, STA, OTHER, &body[..6]));
        dev.queue(&ap_mgmt(mgmt_subtype::ASSOC_RESP, OTHER, AP, &body[..6]));
        drain(&mut a, &mut dev, &mut bufs).expect("both are discarded, not fatal");
        assert_eq!(
            a.phase(),
            Phase::Associating,
            "still waiting for a response from our own AP"
        );
    }

    #[test]
    fn a_data_frame_for_another_station_is_discarded() {
        let (mut a, mut dev, mut bufs) = associate();
        dev.queue(&ap_data(OTHER, AP, OTHER, 0x0800, &[1, 2, 3, 4]));
        let steps = drain(&mut a, &mut dev, &mut bufs).expect("discarded");
        assert!(
            !steps.iter().any(|s| matches!(s, Step::Received { .. })),
            "a frame addressed elsewhere is not handed up: {steps:?}"
        );
    }

    #[test]
    fn a_data_frame_for_us_is_handed_up_as_ethernet() {
        let (mut a, mut dev, mut bufs) = associate();
        let payload = [0xDE, 0xAD, 0xBE, 0xEF];
        dev.queue(&ap_data(STA, AP, OTHER, 0x0800, &payload));
        let steps = drain(&mut a, &mut dev, &mut bufs).expect("received");
        let len = steps
            .iter()
            .find_map(|s| match s {
                Step::Received { len } => Some(*len),
                _ => None,
            })
            .expect("one data frame arrived");
        let eth = bufs.ethernet(len).expect("in range");
        assert_eq!(&eth[..6], &STA);
        assert_eq!(&eth[6..12], &OTHER);
        assert_eq!(&eth[12..14], &0x0800u16.to_be_bytes());
        assert_eq!(&eth[14..], &payload);
    }

    #[test]
    fn a_malformed_frame_is_noise_and_not_a_failure() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();
        dev.queue(&[0x00, 0x01]); // too short for any header
        dev.queue(&auth_response(mgmt::status::SUCCESS));
        dev.queue(&assoc_response(mgmt::status::SUCCESS));
        drain(&mut a, &mut dev, &mut bufs).expect("the noise is skipped");
        assert_eq!(a.phase(), Phase::Handshaking);
    }

    // -- the radio's own failures -------------------------------------------

    #[test]
    fn an_oversized_frame_is_an_error_and_not_a_truncation() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();
        // Longer than any conforming frame, so longer than the receive
        // buffer this driver offers.
        dev.queue(&vec![0u8; MAX_FRAME_LEN + 1]);
        a.poll(&mut dev, &mut bufs)
            .expect("the first poll sends auth");
        let err = a.poll(&mut dev, &mut bufs).expect_err("does not fit");
        assert_eq!(err, Error::Radio(MockError::Oversized(MAX_FRAME_LEN + 1)));
        assert_eq!(dev.rx.len(), 1, "and the frame was not consumed");
    }

    // -- retransmission -----------------------------------------------------

    #[test]
    fn retransmit_resends_whichever_request_is_outstanding() {
        let mut a = association();
        let mut dev = MockRadio::new();
        let mut bufs = Buffers::new();

        a.poll(&mut dev, &mut bufs).expect("auth sent");
        assert!(a.retransmit(&mut dev, &mut bufs).expect("resends"));
        assert_eq!(dev.tx.len(), 2);
        assert_eq!(
            MacHeader::parse(&dev.tx[1]).expect("parses").fc.subtype(),
            mgmt_subtype::AUTH
        );

        dev.queue(&auth_response(mgmt::status::SUCCESS));
        drain(&mut a, &mut dev, &mut bufs).expect("associating");
        let before = dev.tx.len();
        assert!(a.retransmit(&mut dev, &mut bufs).expect("resends"));
        assert_eq!(dev.tx.len(), before + 1);
        assert_eq!(
            MacHeader::parse(&dev.tx[before])
                .expect("parses")
                .fc
                .subtype(),
            mgmt_subtype::ASSOC_REQ
        );
    }

    #[test]
    fn there_is_nothing_to_retransmit_once_the_ap_is_driving() {
        let (mut a, mut dev, mut bufs) = associate();
        assert!(
            !a.retransmit(&mut dev, &mut bufs).expect("nothing pending"),
            "after association the AP drives the handshake"
        );
        assert_eq!(dev.tx.len(), 4, "and nothing went out");
    }

    // -- construction -------------------------------------------------------

    #[test]
    fn an_ssid_longer_than_the_element_that_carries_it_is_refused() {
        let long = [b'x'; MAX_SSID_LEN + 1];
        assert!(Association::new(config(), &long, &BASIC_RATES, CHANNEL, &PMK, SNONCE).is_none());
    }

    #[test]
    fn more_rates_than_the_element_holds_are_refused() {
        let many = [0x82u8; MAX_SUPPORTED_RATES + 1];
        assert!(Association::new(config(), SSID, &many, CHANNEL, &PMK, SNONCE).is_none());
    }

    #[test]
    fn an_unsupported_akm_is_refused_at_construction() {
        // TDLS does not run this handshake at all, so there is no KDF for it
        // and no way to derive a PTK.
        let cfg = Config {
            akm: rsn::akm::TDLS,
            ..config()
        };
        assert!(Association::new(cfg, SSID, &BASIC_RATES, CHANNEL, &PMK, SNONCE).is_none());
    }

    #[test]
    fn debug_prints_the_phase_and_nothing_that_is_secret() {
        let a = association();
        let text = format!("{a:?}");
        assert!(text.contains("Idle"));
        assert!(!text.contains("handshake"));
        assert_eq!(format!("{:?}", Buffers::new()), "Buffers { .. }");
    }
}
