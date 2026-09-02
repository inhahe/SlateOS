//! A simulated WPA2-PSK access point, so a station can associate against
//! something.
//!
//! # Why this exists
//!
//! `net80211::assoc::Association` (lane C) is the station half of a join, and
//! [`crate::net::hwsim`] is the radio it talks through. Neither is an AP.
//! Without one, the station's state machine can be unit-tested against a mock
//! inside lane C's crate but can never be *run*: it sits in
//! `Phase::Authenticating` forever because nothing answers. This module is the
//! other end of the link — the minimum authenticator that will carry a real
//! station from a beacon to an installed pairwise key and a data frame.
//!
//! It is a test fixture, not a product. It serves exactly one station, keeps no
//! timers, and retransmits nothing: over `hwsim` delivery is synchronous and
//! in-memory, so the failure modes a real AP spends most of its code on cannot
//! occur here.
//!
//! # What a green run against this proves, and what it does not
//!
//! It proves **the frame exchange and the key schedule**: that both ends derive
//! the same PTK from the same PMK and nonces, that the MICs each computes
//! verify against the other's, and that the handshake reaches its end with both
//! keys installed exactly once. Lane C's `design-decisions.md` §677 and the
//! `hwsim` module header both say this and it bears repeating here, at the
//! place where someone would be most tempted to overclaim: **`hwsim` does not
//! encrypt.** A frame with the Protected bit set crosses the medium in the
//! clear. Nothing here demonstrates confidentiality, and a green run must not
//! be cited for it.
//!
//! There is a second, subtler limit. The AP side below is built with the same
//! `net80211` writers and parsers the station uses, so for the *frame format*
//! this is in part that crate checked against itself. That is worth stating
//! plainly rather than leaving implied. What rescues the exercise from
//! circularity is the cryptography: the PTK is derived independently on each
//! side from inputs that travelled over the medium, and a MIC computed by one
//! side is verified by the other. Those cannot both be wrong in the same
//! direction and still agree — which is exactly why the association is the
//! interesting test and a frame round-trip alone would not be.
//!
//! # References
//!
//! - IEEE 802.11-2020 §12.7.6 (4-way handshake), §12.7.7 (group key
//!   handshake), §9.3.3 (management frame bodies).
//! - RFC 3394 (AES key wrap), used to wrap the GTK into message 3.

use alloc::vec::Vec;

use net80211::eapol::{self, key_info};
use net80211::frame::{FrameControl, FrameType, MacHeader, SeqCtrl, data_subtype, mgmt_subtype};
use net80211::kdf::{self, Kdf, MicAlgo, Ptk};
use net80211::mgmt;
use net80211::rsn;
use net80211::{MacAddr, llc};

use crate::error::{KernelError, KernelResult};
use crate::net::hwsim::{self, RadioId};

/// The 802.11 header length this module writes: 3 addresses, no QoS, no HT.
const HEADER_LEN: usize = 24;

/// Scratch big enough for any frame this module builds.
const BUF_LEN: usize = 512;

/// The pairwise temporal key length for CCMP-128.
const TK_LEN: usize = 16;

/// Where the AP is in its one conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApPhase {
    /// Beaconing; no station has authenticated.
    Idle,
    /// The station authenticated; waiting for an association request.
    Authenticated,
    /// Associated; message 1 has been sent and message 2 is awaited.
    FourWayM2,
    /// Message 3 has been sent and message 4 is awaited.
    FourWayM4,
    /// The 4-way handshake completed; the link carries data.
    Established,
    /// A group rekey is outstanding.
    Rekeying,
}

/// What one call to [`MockAp::poll`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApStep {
    /// Nothing was waiting.
    Idle,
    /// A frame was consumed, and possibly answered.
    Progressed,
    /// The 4-way handshake just completed. Reported exactly once.
    Established,
    /// A data frame arrived from the station; this is its payload length,
    /// which [`MockAp::last_payload`] returns.
    Data(usize),
}

/// A single-station WPA2-PSK authenticator on a simulated radio.
pub struct MockAp {
    radio: RadioId,
    bssid: MacAddr,
    ssid: Vec<u8>,
    channel: u8,
    pmk: [u8; 32],
    /// Our RSN element *body*, echoed into message 3 where the station
    /// compares it byte-for-byte against what the beacon carried.
    rsn_element: Vec<u8>,
    anonce: [u8; eapol::NONCE_LEN],
    gtk: [u8; TK_LEN],
    gtk_key_id: u8,
    phase: ApPhase,
    sta: Option<MacAddr>,
    ptk: Option<Ptk>,
    replay: u64,
    seq: u16,
    last_payload: Vec<u8>,
    /// How many times the pairwise key was authorised for installation. The
    /// station must cause exactly one.
    m3_sent: u32,
}

impl MockAp {
    /// Build an authenticator on `radio`, advertising `ssid` on `channel`.
    ///
    /// `pmk` is the pairwise master key both sides already share; for WPA2-PSK
    /// that is PBKDF2 over the passphrase and SSID, but the handshake does not
    /// care where it came from, and a boot test that ran 4096 PBKDF2 iterations
    /// to prove a point about key *transport* would be paying a lot for
    /// nothing.
    ///
    /// # Errors
    ///
    /// - [`KernelError::InvalidArgument`] if the SSID is longer than 32 octets
    ///   or the channel is not one the medium accepts.
    pub fn new(
        radio: RadioId,
        ssid: &[u8],
        channel: u8,
        pmk: [u8; 32],
        anonce: [u8; eapol::NONCE_LEN],
        gtk: [u8; TK_LEN],
    ) -> KernelResult<Self> {
        if ssid.len() > 32 || !hwsim::channel_is_valid(channel) {
            return Err(KernelError::InvalidArgument);
        }
        let bssid = hwsim::mac(radio).ok_or(KernelError::NoSuchDevice)?;
        hwsim::set_channel(radio, channel)?;

        Ok(Self {
            radio,
            bssid,
            ssid: ssid.to_vec(),
            channel,
            pmk,
            rsn_element: wpa2_psk_rsn_element()?,
            anonce,
            gtk,
            gtk_key_id: 1,
            phase: ApPhase::Idle,
            sta: None,
            ptk: None,
            replay: 0,
            seq: 0,
            last_payload: Vec::new(),
            m3_sent: 0,
        })
    }

    /// The AP's MAC, which for an infrastructure BSS is also the BSSID.
    #[must_use]
    pub const fn bssid(&self) -> MacAddr {
        self.bssid
    }

    /// Where the conversation has got to.
    #[must_use]
    pub const fn phase(&self) -> ApPhase {
        self.phase
    }

    /// The RSN element body this AP advertises.
    #[must_use]
    pub fn rsn_element(&self) -> &[u8] {
        &self.rsn_element
    }

    /// The payload of the most recent data frame from the station.
    #[must_use]
    pub fn last_payload(&self) -> &[u8] {
        &self.last_payload
    }

    /// How many times message 3 has been sent — i.e. how many times the
    /// station was authorised to install a pairwise key.
    #[must_use]
    pub const fn m3_sent(&self) -> u32 {
        self.m3_sent
    }

    /// The temporal key this side derived, once message 2 has been accepted.
    ///
    /// Exposed so a test can compare it against the station's. That comparison
    /// is the one assertion in the whole exercise that is not the crate being
    /// checked against itself: the two keys were computed independently, from
    /// nonces that crossed the medium, by code that shares only the PMK.
    #[must_use]
    pub fn tk(&self) -> Option<&[u8]> {
        self.ptk.as_ref().map(Ptk::tk)
    }

    /// The group key currently in force, and the slot it occupies.
    #[must_use]
    pub const fn gtk(&self) -> (u8, &[u8; TK_LEN]) {
        (self.gtk_key_id, &self.gtk)
    }

    /// Put one beacon on the medium.
    ///
    /// # Errors
    ///
    /// Propagates a transmit failure from the medium.
    pub fn send_beacon(&mut self) -> KernelResult<()> {
        let mut buf = [0u8; BUF_LEN];
        let hdr_len = self.write_header(
            &mut buf,
            FrameType::Management,
            mgmt_subtype::BEACON,
            net80211::BROADCAST_MAC,
            false,
        )?;

        let mut off = hdr_len;
        mgmt::write_beacon(&mut buf, &mut off, 0, 100, mgmt::capability::PRIVACY)
            .ok_or(KernelError::InternalError)?;
        self.write_common_elements(&mut buf, &mut off)?;

        let frame = buf.get(..off).ok_or(KernelError::InternalError)?;
        hwsim::transmit(self.radio, frame)?;
        Ok(())
    }

    /// Consume at most one frame from the medium and answer it.
    ///
    /// # Errors
    ///
    /// Propagates medium failures, and reports [`KernelError::InvalidArgument`]
    /// for a frame that does not parse or arrives in the wrong phase.
    pub fn poll(&mut self) -> KernelResult<ApStep> {
        let Some(frame) = hwsim::receive(self.radio) else {
            return Ok(ApStep::Idle);
        };
        let hdr = MacHeader::parse(&frame).ok_or(KernelError::InvalidArgument)?;

        // Ignore anything not addressed to us; the medium is shared and a
        // broadcast from another station is noise, not an error.
        if hdr.addr1 != self.bssid && hdr.addr1 != net80211::BROADCAST_MAC {
            return Ok(ApStep::Progressed);
        }
        let from = hdr.addr2.ok_or(KernelError::InvalidArgument)?;
        let body = frame
            .get(hdr.header_len..)
            .ok_or(KernelError::InvalidArgument)?;

        match hdr.fc.frame_type() {
            FrameType::Management => self.on_management(hdr.fc.subtype(), from, body),
            FrameType::Data => self.on_data(from, body),
            _ => Ok(ApStep::Progressed),
        }
    }

    /// Start a group rekey: send group message 1 with a fresh GTK.
    ///
    /// # Errors
    ///
    /// [`KernelError::NotConnected`] if the link is not established.
    pub fn start_rekey(&mut self, new_gtk: [u8; TK_LEN]) -> KernelResult<()> {
        if self.phase != ApPhase::Established {
            return Err(KernelError::NotConnected);
        }
        self.gtk = new_gtk;
        self.gtk_key_id = if self.gtk_key_id == 1 { 2 } else { 1 };
        self.send_group_m1()?;
        self.phase = ApPhase::Rekeying;
        Ok(())
    }

    // -- management ------------------------------------------------------

    fn on_management(&mut self, subtype: u8, from: MacAddr, body: &[u8]) -> KernelResult<ApStep> {
        match subtype {
            mgmt_subtype::AUTH => {
                let Some(mgmt::Body::Auth(auth)) = mgmt::Body::parse(subtype, body) else {
                    return Err(KernelError::InvalidArgument);
                };
                // Open System only; a Shared Key request is refused rather
                // than ignored, so the station fails loudly instead of
                // timing out.
                let status = if auth.algorithm == mgmt::auth_alg::OPEN_SYSTEM {
                    mgmt::status::SUCCESS
                } else {
                    mgmt::status::UNSUPPORTED_AUTH_ALGORITHM
                };
                self.send_auth_resp(from, status)?;
                if status == mgmt::status::SUCCESS {
                    self.sta = Some(from);
                    self.phase = ApPhase::Authenticated;
                }
                Ok(ApStep::Progressed)
            }
            mgmt_subtype::ASSOC_REQ => {
                if self.phase != ApPhase::Authenticated {
                    return Err(KernelError::NotConnected);
                }
                self.send_assoc_resp(from, mgmt::status::SUCCESS)?;
                // The AP sends message 1; the station never does. If this is
                // skipped the station polls Idle forever.
                self.send_m1()?;
                self.phase = ApPhase::FourWayM2;
                Ok(ApStep::Progressed)
            }
            mgmt_subtype::DEAUTH | mgmt_subtype::DISASSOC => {
                self.phase = ApPhase::Idle;
                self.sta = None;
                self.ptk = None;
                Ok(ApStep::Progressed)
            }
            _ => Ok(ApStep::Progressed),
        }
    }

    fn send_auth_resp(&mut self, to: MacAddr, status: u16) -> KernelResult<()> {
        let mut buf = [0u8; BUF_LEN];
        let hdr_len = self.write_header(
            &mut buf,
            FrameType::Management,
            mgmt_subtype::AUTH,
            to,
            false,
        )?;
        let mut off = hdr_len;
        mgmt::write_auth(
            &mut buf,
            &mut off,
            mgmt::auth_alg::OPEN_SYSTEM,
            2, // the authenticator's half of the two-frame Open System exchange
            status,
        )
        .ok_or(KernelError::InternalError)?;
        let frame = buf.get(..off).ok_or(KernelError::InternalError)?;
        hwsim::transmit(self.radio, frame)?;
        Ok(())
    }

    /// Write an association response.
    ///
    /// Hand-built rather than written with a `net80211` helper because there
    /// is no such helper: `net80211` is the station side and only ever parses
    /// this frame. That is a small piece of luck for the test — these octets
    /// are laid out from §9.3.3.7 rather than by the code that will read them,
    /// so the parser is checked against the standard here and not against
    /// itself.
    fn send_assoc_resp(&mut self, to: MacAddr, status: u16) -> KernelResult<()> {
        let mut buf = [0u8; BUF_LEN];
        let hdr_len = self.write_header(
            &mut buf,
            FrameType::Management,
            mgmt_subtype::ASSOC_RESP,
            to,
            false,
        )?;
        let mut off = hdr_len;

        // Capability (2) | Status (2) | AID (2), all little-endian.
        put(&mut buf, &mut off, &mgmt::capability::PRIVACY.to_le_bytes())?;
        put(&mut buf, &mut off, &status.to_le_bytes())?;
        // §9.4.1.8: the two most-significant bits are set to 1 on the wire, so
        // AID 1 is 0xC001. A station that uses the raw value indexes the wrong
        // TIM bit; sending it correctly is what lets that bug be caught.
        put(&mut buf, &mut off, &0xC001u16.to_le_bytes())?;
        self.write_common_elements(&mut buf, &mut off)?;

        let frame = buf.get(..off).ok_or(KernelError::InternalError)?;
        hwsim::transmit(self.radio, frame)?;
        Ok(())
    }

    // -- the 4-way handshake ---------------------------------------------

    fn send_m1(&mut self) -> KernelResult<()> {
        self.replay = self.replay.saturating_add(1);
        let fields = eapol::KeyFrameFields {
            descriptor_type: eapol::descriptor_type::RSN,
            key_info: u16::from(key_info::VERSION_HMAC_SHA1_AES)
                | key_info::PAIRWISE
                | key_info::KEY_ACK,
            key_len: tk_len_field()?,
            replay_counter: self.replay,
            nonce: self.anonce,
            iv: [0u8; eapol::IV_LEN],
            rsc: [0u8; eapol::RSC_LEN],
            key_data: &[],
        };
        // Message 1 carries no MIC: the station cannot have a PTK yet.
        let mut body = [0u8; BUF_LEN];
        let n = eapol::write(&mut body, 2, &fields, eapol::MIC_LEN_DEFAULT)
            .ok_or(KernelError::InternalError)?;
        let sta = self.sta.ok_or(KernelError::NotConnected)?;
        self.send_eapol(sta, body.get(..n).ok_or(KernelError::InternalError)?)
    }

    fn send_m3(&mut self) -> KernelResult<()> {
        let ptk = self.ptk.as_ref().ok_or(KernelError::NotConnected)?;
        let sta = self.sta.ok_or(KernelError::NotConnected)?;

        // Key data: our RSN element, then the GTK KDE, wrapped with the KEK.
        let mut plain = Vec::new();
        plain.extend_from_slice(&[net80211::ie::id::RSN, element_len(&self.rsn_element)?]);
        plain.extend_from_slice(&self.rsn_element);
        append_gtk_kde(&mut plain, self.gtk_key_id, &self.gtk)?;
        // RFC 3394 wraps in 8-octet units; pad with 0xDD (the GTK KDE's own
        // vendor-specific id) per §12.7.2.
        while plain.len() % 8 != 0 {
            plain.push(0xDD);
        }

        let kek = aes::Aes::new(&ptk.kek).map_err(|_| KernelError::InternalError)?;
        let wrapped_len =
            aes::keywrap::wrapped_len(plain.len()).ok_or(KernelError::InternalError)?;
        let mut wrapped = alloc::vec![0u8; wrapped_len];
        aes::keywrap::wrap(&kek, &mut wrapped, &plain).map_err(|_| KernelError::InternalError)?;

        self.replay = self.replay.saturating_add(1);
        let fields = eapol::KeyFrameFields {
            descriptor_type: eapol::descriptor_type::RSN,
            key_info: u16::from(key_info::VERSION_HMAC_SHA1_AES)
                | key_info::PAIRWISE
                | key_info::INSTALL
                | key_info::KEY_ACK
                | key_info::KEY_MIC
                | key_info::SECURE
                | key_info::ENCRYPTED_KEY_DATA,
            key_len: tk_len_field()?,
            replay_counter: self.replay,
            nonce: self.anonce,
            iv: [0u8; eapol::IV_LEN],
            rsc: [0u8; eapol::RSC_LEN],
            key_data: &wrapped,
        };
        let mut body = [0u8; BUF_LEN];
        let n = eapol::write(&mut body, 2, &fields, eapol::MIC_LEN_DEFAULT)
            .ok_or(KernelError::InternalError)?;
        self.mic_in_place(&mut body, n)?;
        self.m3_sent = self.m3_sent.saturating_add(1);
        self.send_eapol(sta, body.get(..n).ok_or(KernelError::InternalError)?)
    }

    fn send_group_m1(&mut self) -> KernelResult<()> {
        let ptk = self.ptk.as_ref().ok_or(KernelError::NotConnected)?;
        let sta = self.sta.ok_or(KernelError::NotConnected)?;

        let mut plain = Vec::new();
        append_gtk_kde(&mut plain, self.gtk_key_id, &self.gtk)?;
        while plain.len() % 8 != 0 {
            plain.push(0xDD);
        }
        let kek = aes::Aes::new(&ptk.kek).map_err(|_| KernelError::InternalError)?;
        let wrapped_len =
            aes::keywrap::wrapped_len(plain.len()).ok_or(KernelError::InternalError)?;
        let mut wrapped = alloc::vec![0u8; wrapped_len];
        aes::keywrap::wrap(&kek, &mut wrapped, &plain).map_err(|_| KernelError::InternalError)?;

        self.replay = self.replay.saturating_add(1);
        // Group message 1 has Key Type clear — that is what makes it a group
        // message rather than a pairwise one.
        let fields = eapol::KeyFrameFields {
            descriptor_type: eapol::descriptor_type::RSN,
            key_info: u16::from(key_info::VERSION_HMAC_SHA1_AES)
                | key_info::KEY_ACK
                | key_info::KEY_MIC
                | key_info::SECURE
                | key_info::ENCRYPTED_KEY_DATA,
            key_len: tk_len_field()?,
            replay_counter: self.replay,
            nonce: [0u8; eapol::NONCE_LEN],
            iv: [0u8; eapol::IV_LEN],
            rsc: [0u8; eapol::RSC_LEN],
            key_data: &wrapped,
        };
        let mut body = [0u8; BUF_LEN];
        let n = eapol::write(&mut body, 2, &fields, eapol::MIC_LEN_DEFAULT)
            .ok_or(KernelError::InternalError)?;
        self.mic_in_place(&mut body, n)?;
        self.send_eapol(sta, body.get(..n).ok_or(KernelError::InternalError)?)
    }

    /// Compute the MIC over the finished frame and write it into the frame.
    fn mic_in_place(&self, body: &mut [u8], n: usize) -> KernelResult<()> {
        let ptk = self.ptk.as_ref().ok_or(KernelError::NotConnected)?;
        let mut mic = [0u8; eapol::MIC_LEN_DEFAULT];
        let frame = body.get(..n).ok_or(KernelError::InternalError)?;
        kdf::compute_mic(
            MicAlgo::HmacSha1,
            &ptk.kck,
            frame,
            eapol::MIC_LEN_DEFAULT,
            &mut mic,
        )
        .ok_or(KernelError::InternalError)?;
        let frame = body.get_mut(..n).ok_or(KernelError::InternalError)?;
        eapol::set_mic(frame, &mic).ok_or(KernelError::InternalError)?;
        Ok(())
    }

    fn on_eapol(&mut self, from: MacAddr, eapol_frame: &[u8]) -> KernelResult<ApStep> {
        let body = eapol::body(eapol_frame).ok_or(KernelError::InvalidArgument)?;
        // The MIC length is not carried in the frame — it is a property of the
        // negotiated AKM, and the parser has to be told which. This AP offers
        // PSK only, so it is always the default 16.
        let key = eapol::KeyFrame::parse(body, eapol::MIC_LEN_DEFAULT)
            .ok_or(KernelError::InvalidArgument)?;

        match self.phase {
            ApPhase::FourWayM2 => {
                // Derive the PTK from the station's SNonce, then verify the
                // MIC it sent. Deriving first is unavoidable — the MIC is
                // keyed by the KCK, which is part of what we are deriving —
                // and it is why a wrong PMK shows up as a MIC failure rather
                // than as anything more specific.
                let ptk = kdf::derive_ptk(
                    Kdf::Sha1,
                    &self.pmk,
                    &self.bssid,
                    &from,
                    &self.anonce,
                    &key.nonce,
                    TK_LEN,
                )
                .ok_or(KernelError::InternalError)?;

                if !kdf::verify_mic(MicAlgo::HmacSha1, &ptk.kck, body, eapol::MIC_LEN_DEFAULT) {
                    // A bad MIC is a wrong passphrase, not a protocol error.
                    // Dropping it silently is what a real AP does; here it
                    // would look like "no progress", so say so.
                    crate::serial_println!("[hwsim-ap]   message 2 MIC did not verify");
                    return Err(KernelError::PermissionDenied);
                }
                self.ptk = Some(ptk);
                self.send_m3()?;
                self.phase = ApPhase::FourWayM4;
                Ok(ApStep::Progressed)
            }
            ApPhase::FourWayM4 => {
                let ptk = self.ptk.as_ref().ok_or(KernelError::NotConnected)?;
                if !kdf::verify_mic(MicAlgo::HmacSha1, &ptk.kck, body, eapol::MIC_LEN_DEFAULT) {
                    crate::serial_println!("[hwsim-ap]   message 4 MIC did not verify");
                    return Err(KernelError::PermissionDenied);
                }
                self.phase = ApPhase::Established;
                Ok(ApStep::Established)
            }
            ApPhase::Rekeying => {
                let ptk = self.ptk.as_ref().ok_or(KernelError::NotConnected)?;
                if !kdf::verify_mic(MicAlgo::HmacSha1, &ptk.kck, body, eapol::MIC_LEN_DEFAULT) {
                    crate::serial_println!("[hwsim-ap]   group message 2 MIC did not verify");
                    return Err(KernelError::PermissionDenied);
                }
                self.phase = ApPhase::Established;
                Ok(ApStep::Progressed)
            }
            _ => Ok(ApStep::Progressed),
        }
    }

    // -- data -------------------------------------------------------------

    fn on_data(&mut self, from: MacAddr, body: &[u8]) -> KernelResult<ApStep> {
        let snap = llc::Snap::parse(body).ok_or(KernelError::InvalidArgument)?;
        if snap.ethertype == llc::ETHERTYPE_EAPOL {
            return self.on_eapol(from, snap.payload);
        }
        self.last_payload.clear();
        self.last_payload.extend_from_slice(snap.payload);
        Ok(ApStep::Data(snap.payload.len()))
    }

    /// Send an Ethernet payload to the station, encapsulated for 802.11.
    ///
    /// # Errors
    ///
    /// [`KernelError::NotConnected`] if no station is associated.
    pub fn send_data(&mut self, ethertype: u16, payload: &[u8]) -> KernelResult<()> {
        let sta = self.sta.ok_or(KernelError::NotConnected)?;
        let protected = self.phase == ApPhase::Established;
        let mut buf = [0u8; BUF_LEN];
        let hdr_len = self.write_header(
            &mut buf,
            FrameType::Data,
            data_subtype::DATA,
            sta,
            protected,
        )?;
        let mut off = hdr_len;
        let n = llc::write_header(
            buf.get_mut(off..).ok_or(KernelError::InternalError)?,
            ethertype,
        )
        .ok_or(KernelError::InternalError)?;
        off = off.checked_add(n).ok_or(KernelError::InternalError)?;
        put(&mut buf, &mut off, payload)?;
        let frame = buf.get(..off).ok_or(KernelError::InternalError)?;
        hwsim::transmit(self.radio, frame)?;
        Ok(())
    }

    fn send_eapol(&mut self, to: MacAddr, eapol_frame: &[u8]) -> KernelResult<()> {
        // EAPOL always crosses unprotected: message 1 and 2 precede the key,
        // and 3 and 4 are protected by their MIC rather than by the cipher.
        let mut buf = [0u8; BUF_LEN];
        let hdr_len =
            self.write_header(&mut buf, FrameType::Data, data_subtype::DATA, to, false)?;
        let mut off = hdr_len;
        let n = llc::write_header(
            buf.get_mut(off..).ok_or(KernelError::InternalError)?,
            llc::ETHERTYPE_EAPOL,
        )
        .ok_or(KernelError::InternalError)?;
        off = off.checked_add(n).ok_or(KernelError::InternalError)?;
        put(&mut buf, &mut off, eapol_frame)?;
        let frame = buf.get(..off).ok_or(KernelError::InternalError)?;
        hwsim::transmit(self.radio, frame)?;
        Ok(())
    }

    // -- shared frame plumbing --------------------------------------------

    fn write_header(
        &mut self,
        buf: &mut [u8],
        ftype: FrameType,
        subtype: u8,
        to: MacAddr,
        protected: bool,
    ) -> KernelResult<usize> {
        let fc = FrameControl::new(ftype, subtype).with_protected(protected);
        // From DS: the AP is the source of everything it sends.
        let fc = if ftype == FrameType::Data {
            fc.with_from_ds(true)
        } else {
            fc
        };
        let header = MacHeader {
            fc,
            duration_id: 0,
            addr1: to,
            addr2: Some(self.bssid),
            addr3: Some(self.bssid),
            seq_ctrl: Some(SeqCtrl::new(self.seq, 0)),
            addr4: None,
            qos_ctrl: None,
            ht_ctrl: None,
            header_len: HEADER_LEN,
        };
        self.seq = self.seq.wrapping_add(1) & 0x0FFF;
        header.write(buf).ok_or(KernelError::InternalError)
    }

    /// SSID, supported rates, DS Parameter Set and the RSN element — the four
    /// a station needs to pick us out of a scan and negotiate a cipher.
    ///
    /// The DS Parameter Set matters more than it looks: `supplicant::scan`
    /// reports the channel as `None` without it, and the call site treats that
    /// as "not a candidate", so omitting it makes the AP invisible for a
    /// reason that has nothing to do with the AP.
    fn write_common_elements(&self, buf: &mut [u8], off: &mut usize) -> KernelResult<()> {
        put(
            buf,
            off,
            &[net80211::ie::id::SSID, element_len(&self.ssid)?],
        )?;
        let ssid = self.ssid.clone();
        put(buf, off, &ssid)?;

        put(buf, off, &[net80211::ie::id::SUPPORTED_RATES, 4])?;
        put(buf, off, &[0x82, 0x84, 0x8B, 0x96])?;

        put(buf, off, &[net80211::ie::id::DS_PARAMETER_SET, 1])?;
        put(buf, off, &[self.channel])?;

        put(
            buf,
            off,
            &[net80211::ie::id::RSN, element_len(&self.rsn_element)?],
        )?;
        let rsn_body = self.rsn_element.clone();
        put(buf, off, &rsn_body)?;
        Ok(())
    }
}

/// The RSN element body for WPA2-PSK with CCMP-128 for both ciphers.
///
/// Written with `rsn::write_body` rather than by hand. The station compares
/// message 3's copy of this element against the beacon's byte-for-byte, so the
/// two must come from one writer; a hand-assembled duplicate here would be a
/// second encoder that only has to disagree in one octet — an omitted PMKID
/// count, a capabilities field the beacon path elides — to make the handshake
/// fail with a mismatch that points at the supplicant rather than at this file.
fn wpa2_psk_rsn_element() -> KernelResult<Vec<u8>> {
    let mut buf = [0u8; 64];
    let mut off = 0usize;
    let n = rsn::write_body(
        &mut buf,
        &mut off,
        rsn::Suite::standard(rsn::cipher::CCMP_128),
        &[rsn::Suite::standard(rsn::cipher::CCMP_128)],
        &[rsn::Suite::standard(rsn::akm::PSK)],
        0,
    )
    .ok_or(KernelError::InternalError)?;
    Ok(buf.get(..n).ok_or(KernelError::InternalError)?.to_vec())
}

/// Append the GTK KDE (00-0F-AC type 1) carrying `gtk` in slot `key_id`.
fn append_gtk_kde(out: &mut Vec<u8>, key_id: u8, gtk: &[u8]) -> KernelResult<()> {
    // KDE: id 0xDD, length, OUI 00-0F-AC, data type 1, then KeyID||Tx||Rsvd
    // followed by the key itself (§12.7.2 Figure 12-42).
    let len = gtk.len().checked_add(6).ok_or(KernelError::InternalError)?;
    out.push(0xDD);
    out.push(u8::try_from(len).map_err(|_| KernelError::InvalidArgument)?);
    out.extend_from_slice(&[0x00, 0x0F, 0xAC, 0x01]);
    out.push(key_id & 0x03);
    out.push(0);
    out.extend_from_slice(gtk);
    Ok(())
}

/// The EAPOL Key Length field, in the width the field has.
///
/// A conversion rather than a cast: `as` would truncate silently, and a key
/// length that did not fit the field is a bug worth reporting rather than one
/// worth halving.
fn tk_len_field() -> KernelResult<u16> {
    u16::try_from(TK_LEN).map_err(|_| KernelError::InternalError)
}

/// The length octet of an information element, refusing one that will not fit.
///
/// An element body longer than 255 octets cannot be expressed at all; a cast
/// would emit a plausible short length and produce a frame that parses into
/// something other than what was written.
fn element_len(body: &[u8]) -> KernelResult<u8> {
    u8::try_from(body.len()).map_err(|_| KernelError::InvalidArgument)
}

/// Copy `src` into `out` at `*off`, advancing it.
fn put(out: &mut [u8], off: &mut usize, src: &[u8]) -> KernelResult<()> {
    let end = off
        .checked_add(src.len())
        .ok_or(KernelError::InternalError)?;
    out.get_mut(*off..end)
        .ok_or(KernelError::InternalError)?
        .copy_from_slice(src);
    *off = end;
    Ok(())
}

// ---------------------------------------------------------------------------
// End-to-end self-test
// ---------------------------------------------------------------------------

/// The channel both radios sit on. Any valid one; 6 is the usual test choice.
const TEST_CHANNEL: u8 = 6;

/// Report a failed expectation and produce the error the test returns.
fn fail(what: &str) -> KernelError {
    crate::serial_println!("[hwsim-ap]   FAIL: {what}");
    KernelError::InternalError
}

/// Drive both state machines until `done` says so, or the bound is reached.
///
/// The bound is the whole timing model.  A real station sleeps between polls
/// and retransmits on a timer; over `hwsim` delivery is synchronous and
/// in-memory, so an `Idle` that persists means one side is not answering, and
/// counting polls is a sharper instrument than a clock would be — it fails at
/// the same place every run rather than at whatever place the machine happened
/// to be slow that day.
fn drive(
    assoc: &mut net80211::assoc::Association<'_>,
    radio: &mut hwsim::HwsimRadio,
    bufs: &mut net80211::assoc::Buffers,
    ap: &mut MockAp,
    what: &str,
    mut done: impl FnMut(&net80211::assoc::Association<'_>, &MockAp) -> bool,
) -> KernelResult<()> {
    use net80211::assoc::Step;

    const MAX_POLLS: usize = 200;
    for _ in 0..MAX_POLLS {
        match assoc.poll(radio, bufs) {
            Ok(Step::Idle | Step::Progressed | Step::Established) => {}
            Ok(Step::Received { len }) => {
                // Data before the caller asked for any is possible, and is the
                // caller's to route or drop.  Here there is nothing to route it
                // to, so note it and carry on.
                crate::serial_println!("[hwsim-ap]   (station received {len} octets of data)");
            }
            Err(e) => {
                crate::serial_println!("[hwsim-ap]   station error during {what}: {e:?}");
                return Err(KernelError::InternalError);
            }
        }
        ap.poll()?;
        if done(assoc, ap) {
            return Ok(());
        }
    }
    Err(fail(what))
}

/// A whole WPA2-PSK join, run rather than mocked.
///
/// Beacon → scan → Open System authentication → association → 4-way handshake
/// → data in both directions → group rekey.  Both ends are real code: lane C's
/// `net80211::assoc::Association` is the station, [`MockAp`] is the
/// authenticator, and [`crate::net::hwsim`] is the medium between them.
///
/// Read the module header before citing a green run for anything: **`hwsim`
/// does not encrypt.**  This proves the frame exchange and the key schedule.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] on the first failed expectation,
/// having printed which one.
#[allow(clippy::too_many_lines)] // One conversation, told once, in order.
pub fn self_test() -> KernelResult<()> {
    use net80211::assoc::{Association, BASIC_RATES, Buffers, Step};
    use net80211::supplicant::{self, Config};

    crate::serial_println!("[hwsim-ap] Running association self-test...");

    // Two radios on one channel: the authenticator and the station.
    let ap_radio = hwsim::create_radio()?;
    let sta_radio_id = hwsim::create_radio()?;
    hwsim::set_up(ap_radio, true)?;
    hwsim::set_up(sta_radio_id, true)?;
    hwsim::set_channel(sta_radio_id, TEST_CHANNEL)?;
    let sta_mac = hwsim::mac(sta_radio_id).ok_or(KernelError::NoSuchDevice)?;

    // A hardcoded PMK.  For WPA2-PSK the real one is PBKDF2 over the
    // passphrase and SSID at 4096 iterations; the handshake cannot tell the
    // difference, and a boot test that spent 4096 HMAC-SHA1 rounds proving a
    // point about key *transport* would be paying a lot for nothing.
    let pmk: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E,
        0x1F, 0x20,
    ];
    let anonce = [0xA5u8; eapol::NONCE_LEN];
    let snonce = [0x5Au8; eapol::NONCE_LEN];
    let gtk1 = [0x11u8; TK_LEN];
    let gtk2 = [0x22u8; TK_LEN];

    let ssid = b"slateos-hwsim";
    let mut ap = MockAp::new(ap_radio, ssid, TEST_CHANNEL, pmk, anonce, gtk1)?;

    // --- 1. Beacon, and a scan that turns it into a candidate --------------
    ap.send_beacon()?;
    let beacon =
        hwsim::receive(sta_radio_id).ok_or_else(|| fail("no beacon reached the station"))?;
    let candidate =
        supplicant::scan(&beacon).ok_or_else(|| fail("beacon is not a scan candidate"))?;
    if candidate.ssid != ssid {
        return Err(fail("beacon carried the wrong SSID"));
    }
    if candidate.bssid != ap.bssid() {
        return Err(fail("beacon carried the wrong BSSID"));
    }
    // Without a DS Parameter Set this is `None` and the AP is invisible for a
    // reason that has nothing to do with the AP — which is why the beacon
    // writes one, and why this is asserted rather than unwrapped quietly.
    let channel = candidate
        .channel
        .ok_or_else(|| fail("beacon carried no DS Parameter Set, so no channel"))?;
    if channel != TEST_CHANNEL {
        return Err(fail("beacon advertised the wrong channel"));
    }
    if !candidate.is_joinable() {
        return Err(fail("candidate is not joinable"));
    }
    let ap_rsn_element = candidate
        .rsn_element
        .ok_or_else(|| fail("beacon carried no RSN element"))?;
    // The element came off the air; compare it against what the AP believes it
    // advertises.  Message 3 is checked against these bytes byte-for-byte, so
    // a beacon writer that mangled a single octet here would surface as a
    // handshake mismatch several steps later, pointing at the supplicant.
    if ap_rsn_element != ap.rsn_element() {
        return Err(fail("the beacon's RSN element is not the one the AP holds"));
    }
    let ap_rsn = candidate
        .rsn
        .ok_or_else(|| fail("the RSN element did not parse"))?;

    // --- 2. Pick the AKM and cipher out of what the AP advertised ----------
    let akm = ap_rsn
        .akm_suites()
        .filter_map(rsn::Suite::standard_type)
        .find(|&t| t == rsn::akm::PSK)
        .ok_or_else(|| fail("the AP does not offer PSK"))?;
    let pairwise = ap_rsn
        .pairwise_ciphers()
        .find(|s| s.standard_type() == Some(rsn::cipher::CCMP_128))
        .unwrap_or_else(|| rsn::Suite::standard(rsn::cipher::CCMP_128));

    // Our own element body.  It must outlive the `Association`: message 3 is
    // checked against these exact bytes, so it cannot live in a receive
    // buffer, which gets reused.
    let sta_rsn = wpa2_psk_rsn_element()?;

    let cfg = Config {
        sta: sta_mac,
        bssid: candidate.bssid,
        akm,
        pairwise,
        sta_rsn_element: &sta_rsn,
        ap_rsn_element,
    };

    // Eight kilobytes, which is too much for a kernel stack — hence the box.
    let mut bufs = alloc::boxed::Box::new(Buffers::new());
    let mut assoc = Association::new(cfg, candidate.ssid, &BASIC_RATES, channel, &pmk, snonce)
        .ok_or_else(|| fail("Association::new refused the configuration"))?;
    let mut radio = hwsim::HwsimRadio::new(sta_radio_id);

    // --- 3. The join ------------------------------------------------------
    drive(
        &mut assoc,
        &mut radio,
        &mut bufs,
        &mut ap,
        "the 4-way handshake did not complete",
        |a, p| a.is_established() && p.phase() == ApPhase::Established,
    )?;
    crate::serial_println!("[hwsim-ap]   Join (beacon → auth → assoc → 4-way): OK");

    // --- 4. Both sides derived the same key -------------------------------
    //
    // This is the assertion that is not `net80211` checked against itself.
    // The two temporal keys were computed independently, from nonces that
    // crossed the medium; agreeing by accident is not available to them.
    let ap_tk = ap.tk().ok_or_else(|| fail("the AP has no PTK"))?;
    if assoc.tk() != ap_tk {
        return Err(fail("station and AP derived different temporal keys"));
    }
    if assoc.tk().len() != TK_LEN {
        return Err(fail("the temporal key is not 16 octets"));
    }
    crate::serial_println!("[hwsim-ap]   Both sides derived the same PTK: OK");

    // --- 5. The GTK arrived, in the slot the AP chose ----------------------
    let (sta_key_id, sta_gtk) = assoc
        .gtk()
        .ok_or_else(|| fail("the station installed no GTK"))?;
    let (ap_key_id, ap_gtk) = ap.gtk();
    if sta_key_id != ap_key_id || sta_gtk != ap_gtk.as_slice() {
        return Err(fail("the station's GTK does not match the AP's"));
    }

    // --- 6. The pairwise key was installed exactly once --------------------
    //
    // The KRACK check, stated the strong way: not "the packet number did not
    // rewind" but "the driver was never asked to reinstall".  `hwsim` would
    // refuse a reinstall, and that refusal is the backstop; a refusal that is
    // never reached is the property that matters.
    let stats = hwsim::stats(sta_radio_id).ok_or(KernelError::NoSuchDevice)?;
    if stats.pairwise_installs != 1 {
        return Err(fail("the pairwise key was not installed exactly once"));
    }
    if stats.key_reinstalls_refused != 0 {
        return Err(fail("a key reinstall was attempted and refused"));
    }
    if !stats.group_installed {
        return Err(fail("no group key was installed"));
    }
    if ap.m3_sent() != 1 {
        return Err(fail("the AP sent message 3 more than once"));
    }
    crate::serial_println!("[hwsim-ap]   Pairwise key installed exactly once, GTK matches: OK");

    // --- 7. Data, station to AP -------------------------------------------
    //
    // An ARP request, because it is the first thing a real station sends and
    // it fits in one frame.  `Association::send` takes an Ethernet II frame
    // and does the 802.11 encapsulation itself.
    const REQUEST: &[u8] = b"arp-who-has-192.0.2.1";
    let mut eth = Vec::new();
    eth.extend_from_slice(&net80211::BROADCAST_MAC);
    eth.extend_from_slice(&sta_mac);
    eth.extend_from_slice(&0x0806u16.to_be_bytes());
    eth.extend_from_slice(REQUEST);
    assoc.send(&mut radio, &mut bufs, &eth).map_err(|e| {
        crate::serial_println!("[hwsim-ap]   station send failed: {e:?}");
        KernelError::InternalError
    })?;
    match ap.poll()? {
        ApStep::Data(n) => {
            // The AP sees the payload *after* the LLC/SNAP header, so what it
            // reports is the Ethernet frame minus its 14-octet header.
            if n != eth.len().saturating_sub(llc::ETHERNET_HEADER_LEN) {
                return Err(fail("the AP received a payload of the wrong length"));
            }
        }
        other => {
            crate::serial_println!("[hwsim-ap]   AP answered {other:?}, expected Data");
            return Err(fail("the AP did not receive the station's data frame"));
        }
    }
    if ap.last_payload() != REQUEST {
        return Err(fail("the AP received the wrong data payload"));
    }
    crate::serial_println!("[hwsim-ap]   Data station → AP: OK");

    // --- 8. Data, AP to station -------------------------------------------
    const REPLY: &[u8] = b"arp-is-at-02:00:00:00:00:01";
    ap.send_data(0x0806, REPLY)?;
    let mut got_reply = false;
    for _ in 0..8 {
        match assoc.poll(&mut radio, &mut bufs) {
            Ok(Step::Received { len }) => {
                let frame = bufs
                    .ethernet(len)
                    .ok_or_else(|| fail("Received length outside the buffer"))?;
                if !frame.ends_with(REPLY) {
                    return Err(fail("the station received the wrong payload"));
                }
                got_reply = true;
                break;
            }
            Ok(_) => {}
            Err(e) => {
                crate::serial_println!("[hwsim-ap]   station error receiving data: {e:?}");
                return Err(KernelError::InternalError);
            }
        }
    }
    if !got_reply {
        return Err(fail("the station never received the AP's data frame"));
    }
    crate::serial_println!("[hwsim-ap]   Data AP → station: OK");

    // --- 9. Group rekey ----------------------------------------------------
    //
    // A rekey installs a *group* key and must not touch the pairwise one.
    // Asserting `pairwise_installs` is unchanged across it is the same KRACK
    // property as step 6, checked at the point where a real implementation is
    // most likely to get it wrong.
    let installs_before = stats.pairwise_installs;
    ap.start_rekey(gtk2)?;
    drive(
        &mut assoc,
        &mut radio,
        &mut bufs,
        &mut ap,
        "the group rekey did not complete",
        |a, p| {
            p.phase() == ApPhase::Established
                && a.gtk()
                    .is_some_and(|(id, k)| id == p.gtk().0 && k == p.gtk().1.as_slice())
        },
    )?;
    let after = hwsim::stats(sta_radio_id).ok_or(KernelError::NoSuchDevice)?;
    if after.pairwise_installs != installs_before {
        return Err(fail("the group rekey reinstalled the pairwise key"));
    }
    if after.key_reinstalls_refused != 0 {
        return Err(fail("the group rekey attempted a refused reinstall"));
    }
    let (id, key) = assoc.gtk().ok_or_else(|| fail("no GTK after the rekey"))?;
    if id == sta_key_id {
        return Err(fail("the rekey reused the old key slot"));
    }
    if key != gtk2.as_slice() {
        return Err(fail("the rekeyed GTK is not the one the AP sent"));
    }
    crate::serial_println!("[hwsim-ap]   Group rekey (new slot, pairwise key untouched): OK");

    // The radios are the test's, not the system's; leaving them attached to
    // the medium would make every later `hwsim` test share a channel with two
    // stations nobody is driving.
    hwsim::destroy_radio(sta_radio_id)?;
    hwsim::destroy_radio(ap_radio)?;

    crate::serial_println!("[hwsim-ap] Association self-test PASSED (9 checks)");
    Ok(())
}
