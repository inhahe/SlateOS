//! The 802.11 MAC header: Frame Control, the address fields, and the
//! variable-length tail (QoS Control, HT Control, a fourth address).
//!
//! IEEE 802.11-2020 §9.2. The header is *variable length* — between 10 and 36
//! octets — and, unusually, you cannot know its length without decoding the
//! two-octet Frame Control field first. Four independent things move the end
//! of the header:
//!
//! | Field | Present when |
//! |---|---|
//! | Address 2, 3 and Sequence Control | not an ACK or CTS |
//! | Address 4 | a data frame with both `to_ds` and `from_ds` (a WDS/mesh link) |
//! | QoS Control | a data frame whose subtype has bit 3 set |
//! | HT Control | the Order/`+HTC` bit is set *and* the frame is QoS data or management |
//!
//! That last row is the one that bites: the Order bit means two different
//! things depending on frame type. On a non-QoS data frame it is the
//! *strictly-ordered* service class and adds nothing to the header; on a QoS
//! data or management frame it is `+HTC` and adds four octets. Reading it as
//! `+HTC` unconditionally shifts the body of every strictly-ordered legacy
//! frame by four bytes.

use crate::{MacAddr, addr_at, le_u16, le_u32};

/// The longest possible MAC header: FC(2) + Duration(2) + 4 addresses(24) +
/// Sequence Control(2) + QoS Control(2) + HT Control(4).
pub const MAX_HEADER_LEN: usize = 36;

/// The shortest possible MAC header: an ACK or CTS — FC(2) + Duration(2) +
/// RA(6).
pub const MIN_HEADER_LEN: usize = 10;

// ---------------------------------------------------------------------------
// Frame Control
// ---------------------------------------------------------------------------

/// The four top-level frame types (IEEE 802.11-2020 §9.2.4.1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// Beacons, probes, authentication, association — everything that manages
    /// the link rather than carrying traffic over it.
    Management,
    /// RTS/CTS/ACK/BlockAck — medium arbitration, mostly handled in firmware.
    Control,
    /// Payload, including the QoS variants and the payload-free Null frames
    /// used to signal power-save transitions.
    Data,
    /// The type added for DMG/S1G (802.11ad/ah), whose frames do not share the
    /// classic header layout at all: the second Frame Control octet is
    /// redefined, and the single address that follows Duration is a BSSID or
    /// SA rather than a receiver address. This crate parses the common
    /// four-octet prefix and one address, then stops — an extension frame's
    /// body is not decoded here, and guessing at Address 2 would fabricate a
    /// transmitter that is not on the wire.
    Extension,
}

impl FrameType {
    /// Decode the two type bits.
    #[must_use]
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => FrameType::Management,
            1 => FrameType::Control,
            2 => FrameType::Data,
            // `bits & 0x03` has exactly four values and the other three are
            // matched above, so this arm is 3 and nothing else.
            _ => FrameType::Extension,
        }
    }

    /// The two-bit encoding.
    #[must_use]
    pub fn bits(self) -> u8 {
        match self {
            FrameType::Management => 0,
            FrameType::Control => 1,
            FrameType::Data => 2,
            FrameType::Extension => 3,
        }
    }
}

/// Management-frame subtypes (§9.2.4.1.3, table 9-1).
pub mod mgmt_subtype {
    /// Association Request.
    pub const ASSOC_REQ: u8 = 0;
    /// Association Response.
    pub const ASSOC_RESP: u8 = 1;
    /// Reassociation Request.
    pub const REASSOC_REQ: u8 = 2;
    /// Reassociation Response.
    pub const REASSOC_RESP: u8 = 3;
    /// Probe Request.
    pub const PROBE_REQ: u8 = 4;
    /// Probe Response.
    pub const PROBE_RESP: u8 = 5;
    /// Timing Advertisement.
    pub const TIMING_ADVERT: u8 = 6;
    /// Beacon.
    pub const BEACON: u8 = 8;
    /// Announcement Traffic Indication Message.
    pub const ATIM: u8 = 9;
    /// Disassociation.
    pub const DISASSOC: u8 = 10;
    /// Authentication.
    pub const AUTH: u8 = 11;
    /// Deauthentication.
    pub const DEAUTH: u8 = 12;
    /// Action.
    pub const ACTION: u8 = 13;
    /// Action No Ack.
    pub const ACTION_NO_ACK: u8 = 14;
}

/// Control-frame subtypes (§9.2.4.1.3, table 9-1).
pub mod ctrl_subtype {
    /// Beamforming Report Poll.
    pub const BEAMFORMING_REPORT_POLL: u8 = 4;
    /// VHT/HE NDP Announcement.
    pub const NDP_ANNOUNCEMENT: u8 = 5;
    /// Control Frame Extension.
    pub const CONTROL_FRAME_EXT: u8 = 6;
    /// Control Wrapper.
    pub const CONTROL_WRAPPER: u8 = 7;
    /// Block Ack Request.
    pub const BLOCK_ACK_REQ: u8 = 8;
    /// Block Ack.
    pub const BLOCK_ACK: u8 = 9;
    /// Power Save Poll.
    pub const PS_POLL: u8 = 10;
    /// Request To Send.
    pub const RTS: u8 = 11;
    /// Clear To Send. Carries only a receiver address.
    pub const CTS: u8 = 12;
    /// Acknowledgement. Carries only a receiver address.
    pub const ACK: u8 = 13;
    /// Contention-Free End.
    pub const CF_END: u8 = 14;
    /// Contention-Free End + Contention-Free Ack.
    pub const CF_END_ACK: u8 = 15;
}

/// Data-frame subtypes (§9.2.4.1.3, table 9-1).
///
/// The encoding is a bitfield rather than an enumeration: bit 2 (`0x04`) means
/// "no payload" (a Null frame) and bit 3 (`0x08`) means "QoS", which is why
/// [`FrameControl::has_qos`] tests a bit rather than listing eight subtypes.
pub mod data_subtype {
    /// Data.
    pub const DATA: u8 = 0;
    /// Data + CF-Ack.
    pub const DATA_CF_ACK: u8 = 1;
    /// Data + CF-Poll.
    pub const DATA_CF_POLL: u8 = 2;
    /// Data + CF-Ack + CF-Poll.
    pub const DATA_CF_ACK_POLL: u8 = 3;
    /// Null (no data). Used to signal power-save state changes.
    pub const NULL: u8 = 4;
    /// CF-Ack (no data).
    pub const CF_ACK: u8 = 5;
    /// CF-Poll (no data).
    pub const CF_POLL: u8 = 6;
    /// CF-Ack + CF-Poll (no data).
    pub const CF_ACK_POLL: u8 = 7;
    /// QoS Data.
    pub const QOS_DATA: u8 = 8;
    /// QoS Data + CF-Ack.
    pub const QOS_DATA_CF_ACK: u8 = 9;
    /// QoS Data + CF-Poll.
    pub const QOS_DATA_CF_POLL: u8 = 10;
    /// QoS Data + CF-Ack + CF-Poll.
    pub const QOS_DATA_CF_ACK_POLL: u8 = 11;
    /// QoS Null (no data).
    pub const QOS_NULL: u8 = 12;
    /// QoS CF-Poll (no data).
    pub const QOS_CF_POLL: u8 = 14;
    /// QoS CF-Ack + CF-Poll (no data).
    pub const QOS_CF_ACK_POLL: u8 = 15;

    /// Bit 3 of the subtype: this is one of the QoS variants.
    pub const QOS_BIT: u8 = 0x08;
    /// Bit 2 of the subtype: this frame carries no payload.
    pub const NO_DATA_BIT: u8 = 0x04;
}

/// The two-octet Frame Control field (§9.2.4.1), held as the little-endian
/// `u16` it is on the wire.
///
/// Bit 0 is the low bit of the *first* octet transmitted, so reading the two
/// octets as a little-endian `u16` puts the standard's bit numbers exactly
/// where the standard says they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameControl(pub u16);

impl FrameControl {
    /// Protocol Version — 0 for every frame defined to date. A non-zero value
    /// means the frame must be discarded, not guessed at.
    #[must_use]
    pub fn version(self) -> u8 {
        (self.0 & 0x0003) as u8
    }

    /// Type.
    #[must_use]
    pub fn frame_type(self) -> FrameType {
        FrameType::from_bits(((self.0 >> 2) & 0x0003) as u8)
    }

    /// Subtype. Interpret against [`mgmt_subtype`], [`ctrl_subtype`] or
    /// [`data_subtype`] according to [`FrameControl::frame_type`].
    #[must_use]
    pub fn subtype(self) -> u8 {
        ((self.0 >> 4) & 0x000F) as u8
    }

    /// To DS: the frame is going from a station towards the distribution
    /// system, i.e. uplink to the AP.
    #[must_use]
    pub fn to_ds(self) -> bool {
        (self.0 & 0x0100) != 0
    }

    /// From DS: the frame is coming out of the distribution system, i.e.
    /// downlink from the AP.
    #[must_use]
    pub fn from_ds(self) -> bool {
        (self.0 & 0x0200) != 0
    }

    /// More Fragments.
    #[must_use]
    pub fn more_fragments(self) -> bool {
        (self.0 & 0x0400) != 0
    }

    /// Retry: this is a retransmission, so a duplicate is expected and must be
    /// dropped rather than delivered twice.
    #[must_use]
    pub fn retry(self) -> bool {
        (self.0 & 0x0800) != 0
    }

    /// Power Management: the sender will be asleep after this frame.
    #[must_use]
    pub fn power_management(self) -> bool {
        (self.0 & 0x1000) != 0
    }

    /// More Data: the AP has further buffered frames for a sleeping station.
    #[must_use]
    pub fn more_data(self) -> bool {
        (self.0 & 0x2000) != 0
    }

    /// Protected Frame: the body is encrypted (WEP/TKIP/CCMP/GCMP) and is
    /// preceded by a cipher-specific IV header.
    #[must_use]
    pub fn protected(self) -> bool {
        (self.0 & 0x4000) != 0
    }

    /// The Order / `+HTC` bit.
    ///
    /// **This bit means two different things.** On a QoS data frame or a
    /// management frame it is `+HTC` and a four-octet HT Control field
    /// follows; on a non-QoS data frame it is the legacy StrictlyOrdered
    /// service class and adds nothing. Use [`FrameControl::has_ht_control`],
    /// which applies that rule, rather than testing this bit directly.
    #[must_use]
    pub fn order(self) -> bool {
        (self.0 & 0x8000) != 0
    }

    /// True if this is a data frame with a QoS Control field.
    #[must_use]
    pub fn has_qos(self) -> bool {
        self.frame_type() == FrameType::Data && (self.subtype() & data_subtype::QOS_BIT) != 0
    }

    /// True if a four-octet HT Control field follows the QoS Control field.
    ///
    /// The Order bit alone is not enough — see [`FrameControl::order`].
    #[must_use]
    pub fn has_ht_control(self) -> bool {
        self.order()
            && match self.frame_type() {
                FrameType::Management => true,
                FrameType::Data => self.has_qos(),
                FrameType::Control | FrameType::Extension => false,
            }
    }

    /// True if a fourth address is present: a data frame bridged between two
    /// APs, where neither the source nor the destination is either endpoint of
    /// the wireless hop.
    #[must_use]
    pub fn has_addr4(self) -> bool {
        self.frame_type() == FrameType::Data && self.to_ds() && self.from_ds()
    }

    /// Build a Frame Control value from its parts. Flags start clear; set them
    /// with the `with_*` builders.
    #[must_use]
    pub fn new(frame_type: FrameType, subtype: u8) -> Self {
        FrameControl((u16::from(frame_type.bits()) << 2) | (u16::from(subtype & 0x0F) << 4))
    }

    /// Return a copy with `to_ds` set to `on`.
    #[must_use]
    pub fn with_to_ds(self, on: bool) -> Self {
        self.with_bit(0x0100, on)
    }

    /// Return a copy with `from_ds` set to `on`.
    #[must_use]
    pub fn with_from_ds(self, on: bool) -> Self {
        self.with_bit(0x0200, on)
    }

    /// Return a copy with `more_fragments` set to `on`.
    #[must_use]
    pub fn with_more_fragments(self, on: bool) -> Self {
        self.with_bit(0x0400, on)
    }

    /// Return a copy with `retry` set to `on`.
    #[must_use]
    pub fn with_retry(self, on: bool) -> Self {
        self.with_bit(0x0800, on)
    }

    /// Return a copy with `power_management` set to `on`.
    #[must_use]
    pub fn with_power_management(self, on: bool) -> Self {
        self.with_bit(0x1000, on)
    }

    /// Return a copy with `more_data` set to `on`.
    #[must_use]
    pub fn with_more_data(self, on: bool) -> Self {
        self.with_bit(0x2000, on)
    }

    /// Return a copy with `protected` set to `on`.
    #[must_use]
    pub fn with_protected(self, on: bool) -> Self {
        self.with_bit(0x4000, on)
    }

    /// Return a copy with the Order / `+HTC` bit set to `on`.
    #[must_use]
    pub fn with_order(self, on: bool) -> Self {
        self.with_bit(0x8000, on)
    }

    fn with_bit(self, mask: u16, on: bool) -> Self {
        FrameControl(if on { self.0 | mask } else { self.0 & !mask })
    }

    /// The two octets as transmitted.
    #[must_use]
    pub fn to_bytes(self) -> [u8; 2] {
        self.0.to_le_bytes()
    }
}

// ---------------------------------------------------------------------------
// Sequence Control
// ---------------------------------------------------------------------------

/// The two-octet Sequence Control field (§9.2.4.4): a 4-bit fragment number
/// and a 12-bit sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeqCtrl(pub u16);

impl SeqCtrl {
    /// Build from a sequence number (wrapped into 12 bits) and a fragment
    /// number (wrapped into 4).
    #[must_use]
    pub fn new(seq: u16, frag: u8) -> Self {
        SeqCtrl(((seq & 0x0FFF) << 4) | u16::from(frag & 0x0F))
    }

    /// The fragment number, 0 for an unfragmented frame.
    #[must_use]
    pub fn fragment(self) -> u8 {
        (self.0 & 0x000F) as u8
    }

    /// The sequence number, which wraps at 4096.
    #[must_use]
    pub fn sequence(self) -> u16 {
        (self.0 >> 4) & 0x0FFF
    }
}

// ---------------------------------------------------------------------------
// The header
// ---------------------------------------------------------------------------

/// The addressing roles of a data frame's address fields, which depend
/// entirely on the `to_ds`/`from_ds` pair (§9.2.4.1.4, table 9-4).
///
/// This is the part of 802.11 that most often gets transcribed wrongly,
/// because Address 1 is *not* the destination in three of the four cases. It
/// is always the **receiver** — the station this hop is aimed at — and the
/// final destination may be somewhere else entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataAddrRoles {
    /// The final destination of the frame.
    pub dst: MacAddr,
    /// The original source of the frame.
    pub src: MacAddr,
    /// The BSSID, absent on a four-address (WDS) frame, which has no single
    /// BSS.
    pub bssid: Option<MacAddr>,
    /// The station receiving this particular hop — always Address 1.
    pub receiver: MacAddr,
    /// The station transmitting this particular hop — always Address 2.
    pub transmitter: MacAddr,
}

/// A parsed 802.11 MAC header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacHeader {
    /// Frame Control.
    pub fc: FrameControl,
    /// Duration/ID: a NAV duration in microseconds, or an association ID on a
    /// PS-Poll. Not interpreted here.
    pub duration_id: u16,
    /// Address 1 — the receiver address. Always present.
    pub addr1: MacAddr,
    /// Address 2 — the transmitter address. Absent on ACK and CTS.
    pub addr2: Option<MacAddr>,
    /// Address 3 — the BSSID, source or destination depending on the DS bits.
    /// Absent on all control frames.
    pub addr3: Option<MacAddr>,
    /// Sequence Control. Absent on all control frames.
    pub seq_ctrl: Option<SeqCtrl>,
    /// Address 4 — present only on a four-address data frame.
    pub addr4: Option<MacAddr>,
    /// QoS Control, present on QoS data subtypes.
    pub qos_ctrl: Option<u16>,
    /// HT Control, present when [`FrameControl::has_ht_control`].
    pub ht_ctrl: Option<u32>,
    /// The length of this header in octets — where the frame body begins.
    pub header_len: usize,
}

impl MacHeader {
    /// Parse a MAC header from the front of `buf`.
    ///
    /// Returns `None` if the buffer is too short for the header the Frame
    /// Control field describes. The frame body is *not* validated: use
    /// [`Frame::parse`] to get the header and body together.
    #[must_use]
    #[allow(clippy::too_many_lines)] // One field per branch; splitting it would
    // hide the running offset that is the whole point of the function.
    pub fn parse(buf: &[u8]) -> Option<Self> {
        let fc = FrameControl(le_u16(buf, 0)?);
        let duration_id = le_u16(buf, 2)?;
        let addr1 = addr_at(buf, 4)?;
        let mut off = 10usize;

        // ACK and CTS stop after the receiver address, and so does an
        // Extension frame as far as this crate is concerned — see
        // [`FrameType::Extension`]. Every other frame, control included,
        // carries at least a transmitter address.
        let short_control = fc.frame_type() == FrameType::Extension
            || (fc.frame_type() == FrameType::Control
                && matches!(fc.subtype(), ctrl_subtype::CTS | ctrl_subtype::ACK));
        if short_control {
            return Some(MacHeader {
                fc,
                duration_id,
                addr1,
                addr2: None,
                addr3: None,
                seq_ctrl: None,
                addr4: None,
                qos_ctrl: None,
                ht_ctrl: None,
                header_len: off,
            });
        }

        let addr2 = Some(addr_at(buf, off)?);
        off = off.checked_add(6)?;

        // Control frames end here: they have no Address 3 and no Sequence
        // Control, because they are never fragmented and never reassembled.
        if fc.frame_type() == FrameType::Control {
            return Some(MacHeader {
                fc,
                duration_id,
                addr1,
                addr2,
                addr3: None,
                seq_ctrl: None,
                addr4: None,
                qos_ctrl: None,
                ht_ctrl: None,
                header_len: off,
            });
        }

        let addr3 = Some(addr_at(buf, off)?);
        off = off.checked_add(6)?;
        let seq_ctrl = Some(SeqCtrl(le_u16(buf, off)?));
        off = off.checked_add(2)?;

        let addr4 = if fc.has_addr4() {
            let a = addr_at(buf, off)?;
            off = off.checked_add(6)?;
            Some(a)
        } else {
            None
        };

        let qos_ctrl = if fc.has_qos() {
            let q = le_u16(buf, off)?;
            off = off.checked_add(2)?;
            Some(q)
        } else {
            None
        };

        let ht_ctrl = if fc.has_ht_control() {
            let h = le_u32(buf, off)?;
            off = off.checked_add(4)?;
            Some(h)
        } else {
            None
        };

        Some(MacHeader {
            fc,
            duration_id,
            addr1,
            addr2,
            addr3,
            seq_ctrl,
            addr4,
            qos_ctrl,
            ht_ctrl,
            header_len: off,
        })
    }

    /// Resolve the addressing roles of a data frame (§9.2.4.1.4, table 9-4).
    ///
    /// Returns `None` for anything that is not a data frame, and for a
    /// three-address frame missing an address the DS bits say should be there.
    /// Management frames do not need this: their Address 1/2/3 are always
    /// destination, source and BSSID.
    #[must_use]
    pub fn data_addr_roles(&self) -> Option<DataAddrRoles> {
        if self.fc.frame_type() != FrameType::Data {
            return None;
        }
        let a1 = self.addr1;
        let a2 = self.addr2?;
        let a3 = self.addr3?;
        Some(match (self.fc.to_ds(), self.fc.from_ds()) {
            // IBSS / peer-to-peer: nobody is a distribution system.
            (false, false) => DataAddrRoles {
                dst: a1,
                src: a2,
                bssid: Some(a3),
                receiver: a1,
                transmitter: a2,
            },
            // Downlink, AP -> station. Address 2 is the AP, and the source is
            // whatever wired host sent the frame in.
            (false, true) => DataAddrRoles {
                dst: a1,
                src: a3,
                bssid: Some(a2),
                receiver: a1,
                transmitter: a2,
            },
            // Uplink, station -> AP. Address 1 is the AP, and the destination
            // is wherever the frame is bound once it leaves the air.
            (true, false) => DataAddrRoles {
                dst: a3,
                src: a2,
                bssid: Some(a1),
                receiver: a1,
                transmitter: a2,
            },
            // AP -> AP (WDS/mesh): the source is in the fourth address, and
            // there is no single BSSID for the hop.
            (true, true) => DataAddrRoles {
                dst: a3,
                src: self.addr4?,
                bssid: None,
                receiver: a1,
                transmitter: a2,
            },
        })
    }

    /// The BSSID a *management* frame belongs to — its Address 3.
    #[must_use]
    pub fn mgmt_bssid(&self) -> Option<MacAddr> {
        if self.fc.frame_type() == FrameType::Management {
            self.addr3
        } else {
            None
        }
    }

    /// Serialise the header into `out`, returning the number of octets
    /// written, or `None` if `out` is shorter than [`MacHeader::header_len`].
    ///
    /// The optional fields are written according to the Frame Control field,
    /// not according to which `Option`s happen to be `Some`: a header whose
    /// `fc` says there is no QoS Control does not grow one because `qos_ctrl`
    /// was left set. A missing field that `fc` requires is an error rather
    /// than a silent zero, because a zeroed Address 2 is a frame from
    /// `00:00:00:00:00:00`, which is a valid-looking lie.
    #[must_use]
    pub fn write(&self, out: &mut [u8]) -> Option<usize> {
        let mut off = 0usize;
        put(out, &mut off, &self.fc.to_bytes())?;
        put(out, &mut off, &self.duration_id.to_le_bytes())?;
        put(out, &mut off, &self.addr1)?;

        let short_control = self.fc.frame_type() == FrameType::Extension
            || (self.fc.frame_type() == FrameType::Control
                && matches!(self.fc.subtype(), ctrl_subtype::CTS | ctrl_subtype::ACK));
        if short_control {
            return Some(off);
        }

        put(out, &mut off, &self.addr2?)?;
        if self.fc.frame_type() == FrameType::Control {
            return Some(off);
        }

        put(out, &mut off, &self.addr3?)?;
        put(
            out,
            &mut off,
            &self.seq_ctrl.unwrap_or_default().0.to_le_bytes(),
        )?;

        if self.fc.has_addr4() {
            put(out, &mut off, &self.addr4?)?;
        }
        if self.fc.has_qos() {
            put(out, &mut off, &self.qos_ctrl.unwrap_or(0).to_le_bytes())?;
        }
        if self.fc.has_ht_control() {
            put(out, &mut off, &self.ht_ctrl.unwrap_or(0).to_le_bytes())?;
        }
        Some(off)
    }
}

/// Copy `src` into `out` at `*off`, advancing `*off`. `None` if it would not
/// fit.
fn put(out: &mut [u8], off: &mut usize, src: &[u8]) -> Option<()> {
    let end = off.checked_add(src.len())?;
    out.get_mut(*off..end)?.copy_from_slice(src);
    *off = end;
    Some(())
}

// ---------------------------------------------------------------------------
// Header + body
// ---------------------------------------------------------------------------

/// A parsed 802.11 frame: its header and the body that follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// The decoded MAC header.
    pub header: MacHeader,
    /// Everything after the header. For a protected frame this still includes
    /// the cipher's IV/header and trailing MIC — decryption is not this
    /// crate's job.
    pub body: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Parse a frame with **no** trailing FCS.
    ///
    /// Most drivers hand up frames with the FCS already checked and stripped
    /// by hardware. If yours does not, use [`Frame::parse_with_fcs`], which
    /// verifies the trailer before parsing — never strip four bytes and call
    /// this, since an unchecked frame is an attacker-shaped frame.
    #[must_use]
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        let header = MacHeader::parse(buf)?;
        let body = buf.get(header.header_len..)?;
        Some(Frame { header, body })
    }

    /// Parse a frame that carries a trailing four-octet FCS, verifying it.
    ///
    /// Returns `None` if the frame is too short, if the header does not
    /// decode, or if the checksum does not match — the three cases are
    /// deliberately not distinguished, because a caller that treats "bad
    /// checksum" differently from "malformed" is one deserialisation bug away
    /// from acting on corrupt data.
    #[must_use]
    pub fn parse_with_fcs(buf: &'a [u8]) -> Option<Self> {
        let stripped = crate::fcs::verify_and_strip(buf)?;
        Frame::parse(stripped)
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

    const AP: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    const STA: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x02];
    const HOST: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x03];
    const AP2: MacAddr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x04];

    fn hdr(fc: FrameControl) -> MacHeader {
        MacHeader {
            fc,
            duration_id: 0x0134,
            addr1: AP,
            addr2: Some(STA),
            addr3: Some(HOST),
            seq_ctrl: Some(SeqCtrl::new(0x123, 2)),
            addr4: Some(AP2),
            qos_ctrl: Some(0xBEEF),
            ht_ctrl: Some(0xDEAD_BEEF),
            header_len: 0,
        }
    }

    fn roundtrip(fc: FrameControl) -> (MacHeader, usize) {
        let mut buf = [0u8; MAX_HEADER_LEN + 8];
        let n = hdr(fc).write(&mut buf).expect("header fits");
        let parsed = MacHeader::parse(&buf).expect("header parses");
        assert_eq!(
            parsed.header_len, n,
            "written length must match parsed length"
        );
        (parsed, n)
    }

    #[test]
    fn frame_control_bits_are_where_the_standard_says() {
        // Beacon, no flags: type 0 subtype 8 -> 0b1000_00_00 = 0x80 in octet 0.
        let fc = FrameControl::new(FrameType::Management, mgmt_subtype::BEACON);
        assert_eq!(fc.to_bytes(), [0x80, 0x00]);
        assert_eq!(fc.frame_type(), FrameType::Management);
        assert_eq!(fc.subtype(), mgmt_subtype::BEACON);
        assert_eq!(fc.version(), 0);

        // QoS data, to-DS: type 2 subtype 8 -> 0x88 in octet 0, ToDS -> 0x01
        // in octet 1. This is the single most common header on a live link.
        let fc = FrameControl::new(FrameType::Data, data_subtype::QOS_DATA).with_to_ds(true);
        assert_eq!(fc.to_bytes(), [0x88, 0x01]);
        assert!(fc.to_ds() && !fc.from_ds() && fc.has_qos());
    }

    #[test]
    fn every_flag_sets_and_clears_independently() {
        let base = FrameControl::new(FrameType::Data, data_subtype::DATA);
        let all = base
            .with_to_ds(true)
            .with_from_ds(true)
            .with_more_fragments(true)
            .with_retry(true)
            .with_power_management(true)
            .with_more_data(true)
            .with_protected(true)
            .with_order(true);
        assert_eq!(
            all.0 & 0xFF00,
            0xFF00,
            "all eight flags live in the second octet"
        );
        assert!(all.to_ds());
        assert!(all.from_ds());
        assert!(all.more_fragments());
        assert!(all.retry());
        assert!(all.power_management());
        assert!(all.more_data());
        assert!(all.protected());
        assert!(all.order());

        // Clearing one must not disturb the type, the subtype, or its
        // neighbours.
        let cleared = all.with_retry(false);
        assert!(!cleared.retry());
        assert!(cleared.more_fragments() && cleared.power_management());
        assert_eq!(cleared.frame_type(), FrameType::Data);
        assert_eq!(cleared.subtype(), data_subtype::DATA);
    }

    #[test]
    fn sequence_control_splits_four_and_twelve_bits() {
        let sc = SeqCtrl::new(4095, 15);
        assert_eq!(sc.sequence(), 4095);
        assert_eq!(sc.fragment(), 15);
        // The sequence number wraps at 4096 rather than bleeding into the
        // fragment number.
        assert_eq!(SeqCtrl::new(4096, 0).sequence(), 0);
        assert_eq!(SeqCtrl::new(0x123, 2).0, 0x1232);
    }

    #[test]
    fn management_header_is_twenty_four_octets() {
        let (h, n) = roundtrip(FrameControl::new(
            FrameType::Management,
            mgmt_subtype::BEACON,
        ));
        assert_eq!(n, 24);
        assert_eq!(h.addr1, AP);
        assert_eq!(h.addr2, Some(STA));
        assert_eq!(h.addr3, Some(HOST));
        assert_eq!(h.seq_ctrl.map(SeqCtrl::sequence), Some(0x123));
        assert_eq!(h.addr4, None);
        assert_eq!(h.qos_ctrl, None);
        assert_eq!(h.ht_ctrl, None);
    }

    #[test]
    fn qos_data_header_is_twenty_six_octets() {
        let (h, n) = roundtrip(FrameControl::new(FrameType::Data, data_subtype::QOS_DATA));
        assert_eq!(n, 26);
        assert_eq!(h.qos_ctrl, Some(0xBEEF));
        assert_eq!(h.ht_ctrl, None);
    }

    #[test]
    fn four_address_qos_frame_with_ht_control_is_the_maximum() {
        let fc = FrameControl::new(FrameType::Data, data_subtype::QOS_DATA)
            .with_to_ds(true)
            .with_from_ds(true)
            .with_order(true);
        let (h, n) = roundtrip(fc);
        assert_eq!(n, MAX_HEADER_LEN);
        assert_eq!(h.addr4, Some(AP2));
        assert_eq!(h.qos_ctrl, Some(0xBEEF));
        assert_eq!(h.ht_ctrl, Some(0xDEAD_BEEF));
    }

    /// The trap in §9.2.4.1.10: on a *non-QoS* data frame the Order bit is the
    /// legacy StrictlyOrdered class and adds no HT Control field. Reading it
    /// as `+HTC` shifts the body of every such frame by four octets.
    #[test]
    fn order_bit_on_a_non_qos_data_frame_adds_no_ht_control() {
        let fc = FrameControl::new(FrameType::Data, data_subtype::DATA).with_order(true);
        assert!(fc.order());
        assert!(!fc.has_ht_control());
        let (h, n) = roundtrip(fc);
        assert_eq!(n, 24);
        assert_eq!(h.ht_ctrl, None);

        // ...whereas on a management frame the same bit does mean +HTC.
        let fc = FrameControl::new(FrameType::Management, mgmt_subtype::ACTION).with_order(true);
        assert!(fc.has_ht_control());
        let (h, n) = roundtrip(fc);
        assert_eq!(n, 28);
        assert_eq!(h.ht_ctrl, Some(0xDEAD_BEEF));
    }

    #[test]
    fn ack_and_cts_carry_only_a_receiver_address() {
        for subtype in [ctrl_subtype::ACK, ctrl_subtype::CTS] {
            let (h, n) = roundtrip(FrameControl::new(FrameType::Control, subtype));
            assert_eq!(n, MIN_HEADER_LEN);
            assert_eq!(h.addr1, AP);
            assert_eq!(h.addr2, None);
            assert_eq!(h.addr3, None);
            assert_eq!(h.seq_ctrl, None);
        }
    }

    /// An extension frame must stop after one address rather than being read
    /// as a management frame — its second Frame Control octet is redefined, so
    /// the DS bits a classic parser would consult mean something else, and a
    /// fabricated Address 2 would name a transmitter that is not on the wire.
    #[test]
    fn extension_frames_stop_after_one_address() {
        let (h, n) = roundtrip(FrameControl::new(FrameType::Extension, 0));
        assert_eq!(n, MIN_HEADER_LEN);
        assert_eq!(h.addr1, AP);
        assert_eq!(h.addr2, None);
        assert_eq!(h.seq_ctrl, None);
        assert!(h.data_addr_roles().is_none());
        assert!(h.mgmt_bssid().is_none());
    }

    #[test]
    fn rts_and_ps_poll_carry_two_addresses_and_no_sequence_control() {
        for subtype in [
            ctrl_subtype::RTS,
            ctrl_subtype::PS_POLL,
            ctrl_subtype::BLOCK_ACK,
        ] {
            let (h, n) = roundtrip(FrameControl::new(FrameType::Control, subtype));
            assert_eq!(n, 16);
            assert_eq!(h.addr2, Some(STA));
            assert_eq!(h.addr3, None);
            assert_eq!(h.seq_ctrl, None);
        }
    }

    #[test]
    fn addressing_roles_follow_the_ds_bits() {
        let base = FrameControl::new(FrameType::Data, data_subtype::DATA);

        // Uplink: Address 1 is the AP, so the destination is Address 3.
        let mut h = hdr(base.with_to_ds(true));
        h.addr1 = AP;
        h.addr2 = Some(STA);
        h.addr3 = Some(HOST);
        let r = h.data_addr_roles().expect("data frame");
        assert_eq!((r.src, r.dst, r.bssid), (STA, HOST, Some(AP)));
        assert_eq!((r.receiver, r.transmitter), (AP, STA));

        // Downlink: Address 1 is the station and Address 2 is the AP, so the
        // *source* is Address 3 — the mirror image, and the case a naive
        // "addr1=dst, addr2=src, addr3=bssid" reading gets wrong.
        let mut h = hdr(base.with_from_ds(true));
        h.addr1 = STA;
        h.addr2 = Some(AP);
        h.addr3 = Some(HOST);
        let r = h.data_addr_roles().expect("data frame");
        assert_eq!((r.src, r.dst, r.bssid), (HOST, STA, Some(AP)));

        // IBSS: no DS at all, and Address 3 is the BSSID.
        let mut h = hdr(base);
        h.addr1 = STA;
        h.addr2 = Some(AP);
        h.addr3 = Some(HOST);
        let r = h.data_addr_roles().expect("data frame");
        assert_eq!((r.src, r.dst, r.bssid), (AP, STA, Some(HOST)));

        // WDS: both DS bits, four addresses, and no BSSID for the hop.
        let mut h = hdr(base.with_to_ds(true).with_from_ds(true));
        h.addr1 = AP2;
        h.addr2 = Some(AP);
        h.addr3 = Some(HOST);
        h.addr4 = Some(STA);
        let r = h.data_addr_roles().expect("data frame");
        assert_eq!((r.src, r.dst, r.bssid), (STA, HOST, None));
        assert_eq!((r.receiver, r.transmitter), (AP2, AP));
    }

    #[test]
    fn addressing_roles_refuse_non_data_frames() {
        let h = hdr(FrameControl::new(
            FrameType::Management,
            mgmt_subtype::BEACON,
        ));
        assert!(h.data_addr_roles().is_none());
        assert_eq!(h.mgmt_bssid(), Some(HOST));

        let h = hdr(FrameControl::new(FrameType::Data, data_subtype::DATA));
        assert!(h.mgmt_bssid().is_none());
    }

    #[test]
    fn truncated_headers_are_rejected_at_every_field_boundary() {
        let fc = FrameControl::new(FrameType::Data, data_subtype::QOS_DATA)
            .with_to_ds(true)
            .with_from_ds(true)
            .with_order(true);
        let mut buf = [0u8; MAX_HEADER_LEN];
        let n = hdr(fc).write(&mut buf).expect("fits");
        assert_eq!(n, MAX_HEADER_LEN);
        for short in 0..MAX_HEADER_LEN {
            assert!(
                MacHeader::parse(&buf[..short]).is_none(),
                "a {short}-octet buffer must not yield a {MAX_HEADER_LEN}-octet header"
            );
        }
        assert!(MacHeader::parse(&buf).is_some());
    }

    #[test]
    fn writing_into_a_short_buffer_fails_rather_than_truncating() {
        let h = hdr(FrameControl::new(
            FrameType::Management,
            mgmt_subtype::BEACON,
        ));
        for short in 0..24 {
            let mut buf = [0u8; 24];
            assert!(
                h.write(&mut buf[..short]).is_none(),
                "{short} octets must not suffice"
            );
        }
        let mut buf = [0u8; 24];
        assert_eq!(h.write(&mut buf), Some(24));
    }

    #[test]
    fn a_header_missing_a_required_address_is_an_error_not_a_zero() {
        let mut h = hdr(FrameControl::new(
            FrameType::Management,
            mgmt_subtype::BEACON,
        ));
        h.addr3 = None;
        let mut buf = [0u8; MAX_HEADER_LEN];
        assert!(
            h.write(&mut buf).is_none(),
            "a missing BSSID must not serialise as 00:00:..."
        );
    }

    #[test]
    fn frame_splits_header_from_body() {
        let fc = FrameControl::new(FrameType::Management, mgmt_subtype::BEACON);
        let mut buf = [0u8; 24 + 4];
        hdr(fc).write(&mut buf).expect("fits");
        buf[24..].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let f = Frame::parse(&buf).expect("parses");
        assert_eq!(f.header.header_len, 24);
        assert_eq!(f.body, &[0xAA, 0xBB, 0xCC, 0xDD]);

        // A frame that is exactly a header has an empty body, not no body.
        let f = Frame::parse(&buf[..24]).expect("parses");
        assert!(f.body.is_empty());
    }

    #[test]
    fn frame_type_bits_round_trip() {
        for t in [
            FrameType::Management,
            FrameType::Control,
            FrameType::Data,
            FrameType::Extension,
        ] {
            assert_eq!(FrameType::from_bits(t.bits()), t);

            // Only the low two bits are consulted. The six above them are the
            // protocol-version and subtype bits, which share the octet and are
            // whatever the sender put there; none of them may shift the answer.
            for high in [0x00u8, 0x04, 0x3C, 0x80, 0xFC] {
                assert_eq!(FrameType::from_bits(high | t.bits()), t);
            }
        }
    }
}
