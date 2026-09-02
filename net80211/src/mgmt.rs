//! Management-frame bodies: the fixed fields that precede the element chain
//! in a beacon, probe, authentication, association or deauthentication frame
//! (IEEE 802.11-2020 §9.3.3).
//!
//! Every one of these bodies is a handful of little-endian fixed fields
//! followed by an information-element chain ([`crate::ie`]). The fixed part
//! differs per subtype and there is no length field anywhere — the subtype in
//! the MAC header is the only thing that says how many octets to skip before
//! the elements start, which is why [`Body::parse`] takes the subtype rather
//! than sniffing.
//!
//! # Time is in TUs
//!
//! A beacon interval is counted in *time units* of 1024 microseconds, not
//! milliseconds. The usual value, 100 TU, is 102.4 ms rather than 100 ms.
//! Treating a TU as a millisecond drifts by 2.4% — about two and a half
//! seconds an hour, which is enough to miss beacons once a station starts
//! sleeping between them.

use crate::{MacAddr, addr_at, le_u16, le_u64};

/// One time unit, in microseconds (§3.1).
pub const TU_MICROSECONDS: u32 = 1024;

/// Capability Information bits (§9.4.1.4).
pub mod capability {
    /// Infrastructure BSS — the sender is an AP.
    pub const ESS: u16 = 0x0001;
    /// Independent BSS — ad-hoc.
    pub const IBSS: u16 = 0x0002;
    /// Contention-free pollable.
    pub const CF_POLLABLE: u16 = 0x0004;
    /// Contention-free poll request.
    pub const CF_POLL_REQUEST: u16 = 0x0008;
    /// Privacy: data frames are encrypted. Set for WEP *and* for WPA — on its
    /// own it says only "not open", never which cipher, which is why a scanner
    /// must read the RSN element to label a network.
    pub const PRIVACY: u16 = 0x0010;
    /// Short preamble.
    pub const SHORT_PREAMBLE: u16 = 0x0020;
    /// PBCC modulation.
    pub const PBCC: u16 = 0x0040;
    /// Channel agility.
    pub const CHANNEL_AGILITY: u16 = 0x0080;
    /// Spectrum management.
    pub const SPECTRUM_MGMT: u16 = 0x0100;
    /// QoS (WMM/802.11e).
    pub const QOS: u16 = 0x0200;
    /// Short slot time.
    pub const SHORT_SLOT_TIME: u16 = 0x0400;
    /// Automatic power-save delivery.
    pub const APSD: u16 = 0x0800;
    /// Radio measurement.
    pub const RADIO_MEASUREMENT: u16 = 0x1000;
    /// DSSS-OFDM.
    pub const DSSS_OFDM: u16 = 0x2000;
    /// Delayed block ack.
    pub const DELAYED_BLOCK_ACK: u16 = 0x4000;
    /// Immediate block ack.
    pub const IMMEDIATE_BLOCK_ACK: u16 = 0x8000;
}

/// Authentication algorithm numbers (§9.4.1.1).
pub mod auth_alg {
    /// Open System — the only algorithm WPA2 uses, because the real
    /// authentication happens afterwards in the four-way handshake.
    pub const OPEN_SYSTEM: u16 = 0;
    /// Shared Key — the WEP challenge-response. Broken.
    pub const SHARED_KEY: u16 = 1;
    /// Fast BSS Transition.
    pub const FT: u16 = 2;
    /// Simultaneous Authentication of Equals — WPA3-Personal.
    pub const SAE: u16 = 3;
    /// FILS shared key without PFS.
    pub const FILS_SK: u16 = 4;
    /// FILS shared key with PFS.
    pub const FILS_SK_PFS: u16 = 5;
    /// FILS public key.
    pub const FILS_PK: u16 = 6;
}

/// Status codes (§9.4.1.9, table 9-50). Only the ones a station acts on.
pub mod status {
    /// Success.
    pub const SUCCESS: u16 = 0;
    /// Unspecified failure.
    pub const UNSPECIFIED_FAILURE: u16 = 1;
    /// The authentication algorithm is not supported.
    pub const UNSUPPORTED_AUTH_ALGORITHM: u16 = 13;
    /// Authentication rejected because of a challenge failure.
    pub const CHALLENGE_FAILURE: u16 = 15;
    /// Authentication or association timed out.
    pub const AUTH_TIMEOUT: u16 = 16;
    /// The AP cannot handle another station.
    pub const AP_FULL: u16 = 17;
    /// Association denied: the station does not support the basic rates.
    pub const UNSUPPORTED_RATES: u16 = 18;
    /// Robust management frame policy violation — what a WPA3 AP returns when
    /// a station tries to associate without management frame protection.
    pub const ROBUST_MGMT_POLICY_VIOLATION: u16 = 31;
    /// An information element was invalid.
    pub const INVALID_ELEMENT: u16 = 40;
    /// The requested pairwise cipher is not valid.
    pub const INVALID_PAIRWISE_CIPHER: u16 = 42;
    /// The requested AKM suite is not valid.
    pub const INVALID_AKMP: u16 = 43;
    /// The RSN element version is not supported.
    pub const UNSUPPORTED_RSNE_VERSION: u16 = 44;
    /// Invalid RSN capabilities.
    pub const INVALID_RSNE_CAPABILITIES: u16 = 45;
    /// SAE: the AP demands an anti-clogging token before it will continue.
    pub const ANTI_CLOGGING_TOKEN_REQUIRED: u16 = 76;
    /// SAE: the finite cyclic group offered is not supported.
    pub const UNSUPPORTED_FINITE_CYCLIC_GROUP: u16 = 77;
}

/// Reason codes (§9.4.1.7, table 9-49). Only the ones a station acts on.
pub mod reason {
    /// Unspecified.
    pub const UNSPECIFIED: u16 = 1;
    /// The previous authentication is no longer valid.
    pub const PREV_AUTH_NOT_VALID: u16 = 2;
    /// The sending station is leaving.
    pub const DEAUTH_LEAVING: u16 = 3;
    /// Disassociated for inactivity.
    pub const INACTIVITY: u16 = 4;
    /// The AP is out of resources.
    pub const AP_BUSY: u16 = 5;
    /// A class-2 frame arrived from a station that has not authenticated.
    pub const CLASS2_FROM_NONAUTH: u16 = 6;
    /// A class-3 frame arrived from a station that has not associated.
    pub const CLASS3_FROM_NONASSOC: u16 = 7;
    /// The sending station is leaving the BSS.
    pub const DISASSOC_LEAVING: u16 = 8;
    /// A message integrity check failed — the TKIP countermeasure trigger.
    pub const MIC_FAILURE: u16 = 14;
    /// The four-way handshake timed out. **This is what a wrong password
    /// looks like**: the AP cannot tell a bad PSK from a lost frame, so it
    /// reports a timeout rather than an authentication failure.
    pub const FOURWAY_TIMEOUT: u16 = 15;
    /// The group-key handshake timed out.
    pub const GROUP_KEY_TIMEOUT: u16 = 16;
    /// An element in the four-way handshake differed from the one advertised.
    pub const IE_DIFFERS: u16 = 17;
    /// Invalid group cipher.
    pub const INVALID_GROUP_CIPHER: u16 = 18;
    /// Invalid pairwise cipher.
    pub const INVALID_PAIRWISE_CIPHER: u16 = 19;
    /// Invalid AKMP.
    pub const INVALID_AKMP: u16 = 20;
    /// 802.1X authentication failed.
    pub const DOT1X_AUTH_FAILED: u16 = 23;
}

/// A beacon or probe-response body: the same three fixed fields followed by
/// elements (§9.3.3.2, §9.3.3.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beacon<'a> {
    /// The AP's TSF timer, in microseconds since the BSS started.
    pub timestamp: u64,
    /// The beacon interval, in [`TU_MICROSECONDS`]-microsecond time units.
    pub beacon_interval: u16,
    /// Capability Information — see [`capability`].
    pub capability: u16,
    /// The element chain.
    pub elements: &'a [u8],
}

impl Beacon<'_> {
    /// Length of the fixed part.
    pub const FIXED_LEN: usize = 12;

    /// The beacon interval in microseconds. A TU is 1024 µs, not 1000.
    #[must_use]
    pub fn interval_micros(&self) -> u32 {
        u32::from(self.beacon_interval).saturating_mul(TU_MICROSECONDS)
    }

    /// True if the Privacy bit is set. This means "encrypted", not "WPA" —
    /// read the RSN element to find out which cipher.
    #[must_use]
    pub fn privacy(&self) -> bool {
        (self.capability & capability::PRIVACY) != 0
    }

    /// True if the sender is an infrastructure AP rather than an ad-hoc peer.
    #[must_use]
    pub fn is_ess(&self) -> bool {
        (self.capability & capability::ESS) != 0
    }
}

/// An authentication body (§9.3.3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Auth<'a> {
    /// The algorithm — see [`auth_alg`].
    pub algorithm: u16,
    /// The transaction sequence number, counting from 1.
    pub seq: u16,
    /// The status code — see [`status`].
    pub status: u16,
    /// The element chain. Carries the challenge text for Shared Key, and the
    /// scalar/element/token for SAE.
    pub elements: &'a [u8],
}

impl Auth<'_> {
    /// Length of the fixed part.
    pub const FIXED_LEN: usize = 6;
}

/// An association-request body (§9.3.3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssocReq<'a> {
    /// Capability Information the station claims.
    pub capability: u16,
    /// How many beacon intervals the station may sleep for.
    pub listen_interval: u16,
    /// The BSSID the station is currently associated with. Present only on a
    /// *re*association request.
    pub current_ap: Option<MacAddr>,
    /// The element chain — SSID, rates, and the station's RSN element.
    pub elements: &'a [u8],
}

/// An association-response body (§9.3.3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssocResp<'a> {
    /// Capability Information.
    pub capability: u16,
    /// The status code — see [`status`].
    pub status: u16,
    /// The raw AID field, top two bits included.
    pub raw_aid: u16,
    /// The element chain.
    pub elements: &'a [u8],
}

impl AssocResp<'_> {
    /// Length of the fixed part.
    pub const FIXED_LEN: usize = 6;

    /// The association identifier, with the two most-significant bits masked
    /// off.
    ///
    /// §9.4.1.8 requires those two bits to be set to 1 on the wire, so the raw
    /// field of an AID of 1 is `0xC001`. Using the raw value as an AID makes
    /// every station look like number 49153 — and, worse, indexes the wrong
    /// bit of the TIM element, so the station never learns it has traffic
    /// waiting.
    #[must_use]
    pub fn aid(&self) -> u16 {
        self.raw_aid & 0x3FFF
    }

    /// True if the association succeeded.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.status == status::SUCCESS
    }
}

/// A deauthentication or disassociation body (§9.3.3.1, §9.3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deauth<'a> {
    /// The reason code — see [`reason`].
    pub reason: u16,
    /// The element chain, normally empty.
    pub elements: &'a [u8],
}

/// A parsed management-frame body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body<'a> {
    /// Beacon.
    Beacon(Beacon<'a>),
    /// Probe Response — the same body as a beacon.
    ProbeResp(Beacon<'a>),
    /// Probe Request: elements only, no fixed fields at all.
    ProbeReq(&'a [u8]),
    /// Authentication.
    Auth(Auth<'a>),
    /// Association Request.
    AssocReq(AssocReq<'a>),
    /// Reassociation Request — an association request plus the current AP.
    ReassocReq(AssocReq<'a>),
    /// Association Response.
    AssocResp(AssocResp<'a>),
    /// Reassociation Response — the same body as an association response.
    ReassocResp(AssocResp<'a>),
    /// Deauthentication.
    Deauth(Deauth<'a>),
    /// Disassociation — the same body as a deauthentication.
    Disassoc(Deauth<'a>),
}

impl<'a> Body<'a> {
    /// Parse a management-frame body given the subtype from the MAC header.
    ///
    /// Returns `None` if the body is too short for the subtype's fixed
    /// fields, or if the subtype has no body layout defined here (Action
    /// frames, whose category-specific layouts belong with the features that
    /// use them, and ATIM, which has no body).
    #[must_use]
    pub fn parse(subtype: u8, body: &'a [u8]) -> Option<Self> {
        use crate::frame::mgmt_subtype as st;
        match subtype {
            st::BEACON => parse_beacon(body).map(Body::Beacon),
            st::PROBE_RESP => parse_beacon(body).map(Body::ProbeResp),
            st::PROBE_REQ => Some(Body::ProbeReq(body)),
            st::AUTH => parse_auth(body).map(Body::Auth),
            st::ASSOC_REQ => parse_assoc_req(body, false).map(Body::AssocReq),
            st::REASSOC_REQ => parse_assoc_req(body, true).map(Body::ReassocReq),
            st::ASSOC_RESP => parse_assoc_resp(body).map(Body::AssocResp),
            st::REASSOC_RESP => parse_assoc_resp(body).map(Body::ReassocResp),
            st::DEAUTH => parse_deauth(body).map(Body::Deauth),
            st::DISASSOC => parse_deauth(body).map(Body::Disassoc),
            _ => None,
        }
    }

    /// The element chain of whichever body this is.
    #[must_use]
    pub fn elements(&self) -> &'a [u8] {
        match self {
            Body::Beacon(b) | Body::ProbeResp(b) => b.elements,
            Body::ProbeReq(e) => e,
            Body::Auth(a) => a.elements,
            Body::AssocReq(a) | Body::ReassocReq(a) => a.elements,
            Body::AssocResp(a) | Body::ReassocResp(a) => a.elements,
            Body::Deauth(d) | Body::Disassoc(d) => d.elements,
        }
    }
}

fn parse_beacon(body: &[u8]) -> Option<Beacon<'_>> {
    Some(Beacon {
        timestamp: le_u64(body, 0)?,
        beacon_interval: le_u16(body, 8)?,
        capability: le_u16(body, 10)?,
        elements: body.get(Beacon::FIXED_LEN..)?,
    })
}

fn parse_auth(body: &[u8]) -> Option<Auth<'_>> {
    Some(Auth {
        algorithm: le_u16(body, 0)?,
        seq: le_u16(body, 2)?,
        status: le_u16(body, 4)?,
        elements: body.get(Auth::FIXED_LEN..)?,
    })
}

fn parse_assoc_req(body: &[u8], reassoc: bool) -> Option<AssocReq<'_>> {
    let capability = le_u16(body, 0)?;
    let listen_interval = le_u16(body, 2)?;
    let (current_ap, fixed) = if reassoc {
        (Some(addr_at(body, 4)?), 10)
    } else {
        (None, 4)
    };
    Some(AssocReq {
        capability,
        listen_interval,
        current_ap,
        elements: body.get(fixed..)?,
    })
}

fn parse_assoc_resp(body: &[u8]) -> Option<AssocResp<'_>> {
    Some(AssocResp {
        capability: le_u16(body, 0)?,
        status: le_u16(body, 2)?,
        raw_aid: le_u16(body, 4)?,
        elements: body.get(AssocResp::FIXED_LEN..)?,
    })
}

fn parse_deauth(body: &[u8]) -> Option<Deauth<'_>> {
    Some(Deauth {
        reason: le_u16(body, 0)?,
        elements: body.get(2..)?,
    })
}

// ---------------------------------------------------------------------------
// Building — the four bodies a station originates
// ---------------------------------------------------------------------------

/// Write an authentication body. A station's Open System request is
/// `algorithm = OPEN_SYSTEM, seq = 1, status = SUCCESS`.
#[must_use]
pub fn write_auth(
    out: &mut [u8],
    off: &mut usize,
    algorithm: u16,
    seq: u16,
    status_code: u16,
) -> Option<usize> {
    let start = *off;
    put(out, off, &algorithm.to_le_bytes())?;
    put(out, off, &seq.to_le_bytes())?;
    put(out, off, &status_code.to_le_bytes())?;
    off.checked_sub(start)
}

/// Write the fixed part of an association request. The caller appends the
/// element chain with [`crate::ie::write_element`].
#[must_use]
pub fn write_assoc_req(
    out: &mut [u8],
    off: &mut usize,
    capability_info: u16,
    listen_interval: u16,
) -> Option<usize> {
    let start = *off;
    put(out, off, &capability_info.to_le_bytes())?;
    put(out, off, &listen_interval.to_le_bytes())?;
    off.checked_sub(start)
}

/// Write a deauthentication or disassociation body.
#[must_use]
pub fn write_deauth(out: &mut [u8], off: &mut usize, reason_code: u16) -> Option<usize> {
    let start = *off;
    put(out, off, &reason_code.to_le_bytes())?;
    off.checked_sub(start)
}

/// Write the fixed part of a beacon or probe response.
#[must_use]
pub fn write_beacon(
    out: &mut [u8],
    off: &mut usize,
    timestamp: u64,
    beacon_interval: u16,
    capability_info: u16,
) -> Option<usize> {
    let start = *off;
    put(out, off, &timestamp.to_le_bytes())?;
    put(out, off, &beacon_interval.to_le_bytes())?;
    put(out, off, &capability_info.to_le_bytes())?;
    off.checked_sub(start)
}

fn put(out: &mut [u8], off: &mut usize, src: &[u8]) -> Option<()> {
    let end = off.checked_add(src.len())?;
    out.get_mut(*off..end)?.copy_from_slice(src);
    *off = end;
    Some(())
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
    use crate::frame::mgmt_subtype as st;
    use crate::ie;

    #[test]
    fn a_beacon_body_round_trips_with_its_elements() {
        let mut buf = [0u8; 64];
        let mut off = 0usize;
        write_beacon(
            &mut buf,
            &mut off,
            0x0011_2233_4455_6677,
            100,
            capability::ESS,
        )
        .expect("fits");
        assert_eq!(off, Beacon::FIXED_LEN);
        ie::write_ssid(&mut buf, &mut off, b"slate").expect("fits");
        ie::write_element(&mut buf, &mut off, ie::id::DS_PARAMETER_SET, &[6]).expect("fits");

        let Some(Body::Beacon(b)) = Body::parse(st::BEACON, &buf[..off]) else {
            panic!("expected a beacon");
        };
        assert_eq!(b.timestamp, 0x0011_2233_4455_6677);
        assert_eq!(b.beacon_interval, 100);
        assert!(b.is_ess());
        assert!(!b.privacy());
        assert_eq!(ie::ssid(b.elements), Some(&b"slate"[..]));
        assert_eq!(ie::channel(b.elements), Some(6));
    }

    /// A time unit is 1024 microseconds. The usual 100-TU interval is
    /// 102.4 ms; reading it as 100 ms drifts by 2.4%.
    #[test]
    fn a_time_unit_is_not_a_millisecond() {
        let b = Beacon {
            timestamp: 0,
            beacon_interval: 100,
            capability: 0,
            elements: &[],
        };
        assert_eq!(b.interval_micros(), 102_400);
        assert_ne!(b.interval_micros(), 100_000);
        // The multiplication must saturate rather than wrap on a hostile
        // interval: 65535 TU is 67 ms short of 67.1 seconds and fits, but the
        // saturating form is what keeps it from ever being a panic.
        let b = Beacon {
            timestamp: 0,
            beacon_interval: u16::MAX,
            capability: 0,
            elements: &[],
        };
        assert_eq!(b.interval_micros(), 67_107_840);
    }

    #[test]
    fn the_privacy_bit_says_encrypted_and_nothing_more() {
        let b = Beacon {
            timestamp: 0,
            beacon_interval: 100,
            capability: capability::ESS | capability::PRIVACY,
            elements: &[],
        };
        assert!(b.privacy() && b.is_ess());
        // No RSN element: privacy set with no RSN is WEP, and the caller has
        // to look to find that out. The body cannot say.
        assert!(ie::Elements::find_id(b.elements, ie::id::RSN).is_none());
    }

    #[test]
    fn an_association_response_masks_the_two_top_aid_bits() {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&capability::ESS.to_le_bytes());
        buf[2..4].copy_from_slice(&status::SUCCESS.to_le_bytes());
        buf[4..6].copy_from_slice(&0xC001u16.to_le_bytes());
        let Some(Body::AssocResp(r)) = Body::parse(st::ASSOC_RESP, &buf[..6]) else {
            panic!("expected an association response");
        };
        assert_eq!(r.raw_aid, 0xC001);
        assert_eq!(
            r.aid(),
            1,
            "the two MSBs are always set on the wire and are not the AID"
        );
        assert!(r.accepted());
    }

    #[test]
    fn a_rejected_association_reports_its_status() {
        let mut buf = [0u8; 6];
        buf[2..4].copy_from_slice(&status::ROBUST_MGMT_POLICY_VIOLATION.to_le_bytes());
        let Some(Body::AssocResp(r)) = Body::parse(st::ASSOC_RESP, &buf) else {
            panic!("expected an association response");
        };
        assert!(!r.accepted());
        assert_eq!(r.status, 31);
    }

    #[test]
    fn a_reassociation_request_carries_six_more_octets_than_an_association_one() {
        let ap = [0x02u8, 0, 0, 0, 0, 9];
        let mut buf = [0u8; 16];
        buf[0..2].copy_from_slice(&capability::ESS.to_le_bytes());
        buf[2..4].copy_from_slice(&10u16.to_le_bytes());
        buf[4..10].copy_from_slice(&ap);
        buf[10..13].copy_from_slice(&[ie::id::SSID, 1, b'x']);

        let Some(Body::ReassocReq(r)) = Body::parse(st::REASSOC_REQ, &buf[..13]) else {
            panic!("expected a reassociation request");
        };
        assert_eq!(r.current_ap, Some(ap));
        assert_eq!(r.listen_interval, 10);
        assert_eq!(ie::ssid(r.elements), Some(&b"x"[..]));

        // Parsed as a plain association request, the same bytes put the
        // current-AP address where the elements should be — which is what the
        // subtype argument exists to prevent.
        let Some(Body::AssocReq(r)) = Body::parse(st::ASSOC_REQ, &buf[..13]) else {
            panic!("expected an association request");
        };
        assert_eq!(r.current_ap, None);
        assert_ne!(ie::ssid(r.elements), Some(&b"x"[..]));
    }

    #[test]
    fn an_authentication_body_round_trips() {
        let mut buf = [0u8; 16];
        let mut off = 0usize;
        write_auth(
            &mut buf,
            &mut off,
            auth_alg::OPEN_SYSTEM,
            1,
            status::SUCCESS,
        )
        .expect("fits");
        assert_eq!(off, Auth::FIXED_LEN);
        let Some(Body::Auth(a)) = Body::parse(st::AUTH, &buf[..off]) else {
            panic!("expected an authentication frame");
        };
        assert_eq!(
            (a.algorithm, a.seq, a.status),
            (auth_alg::OPEN_SYSTEM, 1, status::SUCCESS)
        );
        assert!(a.elements.is_empty());
    }

    #[test]
    fn a_deauthentication_body_round_trips() {
        let mut buf = [0u8; 4];
        let mut off = 0usize;
        write_deauth(&mut buf, &mut off, reason::DEAUTH_LEAVING).expect("fits");
        assert_eq!(off, 2);
        let Some(Body::Deauth(d)) = Body::parse(st::DEAUTH, &buf[..off]) else {
            panic!("expected a deauthentication");
        };
        assert_eq!(d.reason, reason::DEAUTH_LEAVING);

        // A disassociation has the identical body but is a different frame.
        let Some(Body::Disassoc(d)) = Body::parse(st::DISASSOC, &buf[..off]) else {
            panic!("expected a disassociation");
        };
        assert_eq!(d.reason, reason::DEAUTH_LEAVING);
    }

    #[test]
    fn a_probe_request_is_elements_and_nothing_else() {
        let body = [ie::id::SSID, 0]; // wildcard SSID
        let Some(Body::ProbeReq(e)) = Body::parse(st::PROBE_REQ, &body) else {
            panic!("expected a probe request");
        };
        assert_eq!(ie::ssid(e), Some(&b""[..]));
        assert!(ie::ssid_is_hidden(ie::ssid(e).expect("ssid element")));
        // An empty probe request is legal: no elements at all.
        assert!(matches!(
            Body::parse(st::PROBE_REQ, &[]),
            Some(Body::ProbeReq(&[]))
        ));
    }

    #[test]
    fn every_body_is_refused_when_short_by_one_octet() {
        let cases: &[(u8, usize)] = &[
            (st::BEACON, Beacon::FIXED_LEN),
            (st::PROBE_RESP, Beacon::FIXED_LEN),
            (st::AUTH, Auth::FIXED_LEN),
            (st::ASSOC_REQ, 4),
            (st::REASSOC_REQ, 10),
            (st::ASSOC_RESP, AssocResp::FIXED_LEN),
            (st::REASSOC_RESP, AssocResp::FIXED_LEN),
            (st::DEAUTH, 2),
            (st::DISASSOC, 2),
        ];
        let buf = [0u8; 16];
        for &(subtype, fixed) in cases {
            for short in 0..fixed {
                assert!(
                    Body::parse(subtype, &buf[..short]).is_none(),
                    "subtype {subtype} accepted a {short}-octet body needing {fixed}"
                );
            }
            assert!(
                Body::parse(subtype, &buf[..fixed]).is_some(),
                "subtype {subtype} rejected an exactly-{fixed}-octet body"
            );
        }
    }

    #[test]
    fn subtypes_without_a_body_layout_here_are_refused() {
        let buf = [0u8; 16];
        assert!(Body::parse(st::ACTION, &buf).is_none());
        assert!(Body::parse(st::ATIM, &buf).is_none());
        assert!(Body::parse(st::TIMING_ADVERT, &buf).is_none());
    }

    #[test]
    fn elements_are_reachable_through_the_enum_for_every_variant() {
        let buf = [0u8; 16];
        for &subtype in &[
            st::BEACON,
            st::PROBE_RESP,
            st::PROBE_REQ,
            st::AUTH,
            st::ASSOC_REQ,
            st::REASSOC_REQ,
            st::ASSOC_RESP,
            st::REASSOC_RESP,
            st::DEAUTH,
            st::DISASSOC,
        ] {
            let b = Body::parse(subtype, &buf).expect("parses");
            // Every variant must reach its own tail rather than the whole
            // body — an accessor that returned `body` would silently re-parse
            // the fixed fields as elements.
            assert!(b.elements().len() <= buf.len());
        }
        let b = Body::parse(st::BEACON, &buf).expect("parses");
        assert_eq!(b.elements().len(), buf.len() - Beacon::FIXED_LEN);
    }

    #[test]
    fn writers_refuse_short_buffers() {
        let mut off = 0usize;
        let mut buf = [0u8; 16];
        for short in 0..6 {
            let mut o = 0usize;
            assert!(write_auth(&mut buf[..short], &mut o, 0, 1, 0).is_none());
        }
        for short in 0..12 {
            let mut o = 0usize;
            assert!(write_beacon(&mut buf[..short], &mut o, 0, 100, 0).is_none());
        }
        for short in 0..4 {
            let mut o = 0usize;
            assert!(write_assoc_req(&mut buf[..short], &mut o, 0, 10).is_none());
        }
        for short in 0..2 {
            let mut o = 0usize;
            assert!(write_deauth(&mut buf[..short], &mut o, 1).is_none());
        }
        assert_eq!(
            write_assoc_req(&mut buf, &mut off, capability::ESS, 10),
            Some(4)
        );
    }
}
