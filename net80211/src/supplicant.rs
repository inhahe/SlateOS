//! The station side of joining a protected network: pick a BSS, ask to join
//! it, run the 4-way handshake, then carry Ethernet frames over the link.
//!
//! Everything above this module in the crate is *shape* — where a field sits
//! and how wide it is. This module is the first that has an opinion about
//! **order**: which message may arrive when, what must be true before its
//! contents are believed, and what may be done twice. That is the whole
//! subject matter of the 4-way handshake, and it is where the attacks live.
//!
//! ## Why the frame layer alone is not enough
//!
//! A parser that accepts a well-formed message 3 is not a supplicant. Every
//! one of the following is a well-formed message 3, and a supplicant that
//! treats them alike is broken in a way no amount of frame-level testing
//! finds:
//!
//! - one whose MIC does not verify — an attacker's, and it must be dropped
//!   *before* anything inside it is read, including the key data;
//! - one whose replay counter has been seen — a recording, played back;
//! - one whose ANonce is not the ANonce message 1 carried — a splice of two
//!   handshakes;
//! - one whose RSN element is not byte-for-byte the one the beacon
//!   advertised — a downgrade, which is the specific thing the handshake
//!   exists to detect;
//! - one that is a genuine retransmission of a message 3 already answered,
//!   because message 4 was lost. This one must be *answered* and must not
//!   cause the key to be installed a second time.
//!
//! That last case is KRACK (Vanhoef & Piessens, 2017). Installing a key
//! resets the packet number the cipher uses as a nonce; installing the same
//! key twice therefore replays the nonce, and CCMP with a repeated nonce
//! leaks the keystream. The defence is not cryptographic — it is a state
//! variable. [`Outcome::Retransmission`] exists so that the distinction is
//! visible in the return type rather than living in a comment: a caller that
//! installs keys on [`Outcome::Complete`] and merely transmits on
//! [`Outcome::Retransmission`] cannot get this wrong by omission.
//!
//! ## What this module does not do
//!
//! No randomness, no clock, no I/O, and no allocation. The station's nonce is
//! supplied by the caller ([`Handshake::new`]) rather than generated here:
//! this crate links into a kernel driver with no entropy source of its own,
//! and a hard-coded fallback RNG in a `no_std` crypto path is exactly the
//! shape of bug that ships. It also makes every test below deterministic.
//! Timeouts and retransmission of *our* messages are the caller's job too —
//! they need a clock, and the state machine does not.
//!
//! ## References
//!
//! - IEEE Std 802.11-2020 §12.7.6 (4-way handshake), §12.7.7 (group key
//!   handshake), §12.7.2 (Key Data and KDEs).
//! - Vanhoef & Piessens, *Key Reinstallation Attacks* (CCS 2017).

use crate::MacAddr;
use crate::eapol::{self, KeyFrameFields, Message, key_info};
use crate::frame::{self, FrameControl, FrameType, MacHeader, SeqCtrl, mgmt_subtype};
use crate::ie;
use crate::kdf::{self, Kdf, KeyData, MicAlgo, NONCE_LEN, Ptk};
use crate::llc;
use crate::mgmt;
use crate::rsn::{self, Rsn};

/// The largest Key Data field this module will unwrap.
///
/// Message 3 carries an RSN element and a GTK KDE, which together run to well
/// under a hundred octets; 256 leaves room for the IGTK, a second RSN element
/// and vendor KDEs without being large enough to matter on a kernel stack.
pub const MAX_KEY_DATA_LEN: usize = 256;

/// The largest group key: 32 octets, for a 256-bit GCMP key.
pub const MAX_GTK_LEN: usize = 32;

/// A PMK is always 256 bits, whatever produced it.
pub const PMK_LEN: usize = 32;

/// Everything that can stop a message being accepted.
///
/// Deliberately fine-grained. A supplicant that reports one `Invalid` for all
/// of these is unusable in the field, because the operator's next question is
/// always "wrong password, or wrong network, or someone in the middle?" — and
/// those are [`Error::BadMic`], [`Error::RsnMismatch`] and
/// [`Error::AnonceChanged`] respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not a well-formed EAPOL-Key frame, or not an EAPOL-Key frame at all.
    Malformed,
    /// Well-formed, but not a message this state machine handles — a message
    /// 1 or 3 arriving at the authenticator's side, a Request, a MIC-failure
    /// report.
    Unexpected,
    /// A valid message, but not one this state accepts.
    WrongState,
    /// The replay counter is not greater than the last one accepted. Almost
    /// always a recording being played back.
    Replay,
    /// The MIC does not verify. On an initial association this is what a
    /// wrong passphrase looks like — the PMK differs, so the KCK differs, so
    /// message 2's MIC is wrong and the AP stops answering. There is no
    /// distinct "wrong password" signal in the protocol.
    BadMic,
    /// Message 3's ANonce is not the one message 1 carried, so the two are
    /// from different handshakes and the PTK derived from the first does not
    /// belong to the second.
    AnonceChanged,
    /// The RSN element in message 3 is not byte-for-byte the one the beacon
    /// advertised: someone edited the beacon to offer a weaker cipher.
    RsnMismatch,
    /// The AKM or cipher suite is not one this stack implements.
    Unsupported,
    /// Key Data is longer than [`MAX_KEY_DATA_LEN`], or is not a multiple of
    /// the key-wrap semiblock, or its integrity check failed on unwrap.
    BadKeyData,
    /// Message 3 or group message 1 carried no GTK KDE.
    NoGroupKey,
    /// The caller's output buffer is too small for the reply.
    OutputTooSmall,
}

/// What accepting a message means for the caller.
///
/// The three variants differ only in what the caller must do *besides*
/// transmitting the reply, which is why they are three variants and not a
/// length plus a boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Transmit `len` octets from the output buffer. Nothing else to do.
    Reply {
        /// Octets written to the caller's buffer.
        len: usize,
    },
    /// Transmit `len` octets, **and install the keys** now available from
    /// [`Handshake::tk`] and [`Handshake::gtk`]. This is returned exactly
    /// once per key.
    Complete {
        /// Octets written to the caller's buffer.
        len: usize,
    },
    /// Transmit `len` octets and **do nothing else**. The peer did not hear
    /// our last reply and has asked again; the keys are already installed and
    /// must not be installed again. See the module docs on KRACK.
    Retransmission {
        /// Octets written to the caller's buffer.
        len: usize,
    },
}

impl Outcome {
    /// The number of octets written to the caller's output buffer.
    #[must_use]
    pub fn len(self) -> usize {
        match self {
            Outcome::Reply { len }
            | Outcome::Complete { len }
            | Outcome::Retransmission { len } => len,
        }
    }

    /// True if no reply is to be transmitted.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// True only for the one outcome that authorises installing a key.
    #[must_use]
    pub fn installs_keys(self) -> bool {
        matches!(self, Outcome::Complete { .. })
    }
}

/// Where in the handshake we are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Nothing received yet; only message 1 is acceptable.
    AwaitingM1,
    /// Message 2 sent, PTK derived but not installed; only message 3 is
    /// acceptable.
    AwaitingM3,
    /// Message 4 sent and the keys installed. Message 3 may still arrive as a
    /// retransmission, and group message 1 may arrive at any time thereafter.
    Established,
}

/// The parameters of one association, fixed at the moment the station decides
/// to join and unchanged for the life of the link.
///
/// The two RSN elements are borrowed rather than copied because the caller
/// necessarily holds them already — it sent one and received the other — and
/// because copying them would mean choosing a maximum length for a field the
/// standard allows to be 255 octets.
#[derive(Debug, Clone, Copy)]
pub struct Config<'a> {
    /// The station's own MAC address (the SPA).
    pub sta: MacAddr,
    /// The AP's MAC address (the AA), which for an infrastructure BSS is also
    /// the BSSID.
    pub bssid: MacAddr,
    /// The negotiated AKM suite type, from [`rsn::akm`].
    pub akm: u8,
    /// The negotiated pairwise cipher suite.
    pub pairwise: rsn::Suite,
    /// The RSN element *body* the station put in its association request —
    /// echoed verbatim in message 2 so the AP can check we were not
    /// downgraded either.
    pub sta_rsn_element: &'a [u8],
    /// The RSN element *body* from the AP's beacon or probe response. Message
    /// 3's copy must match it exactly.
    pub ap_rsn_element: &'a [u8],
}

/// The station side of the 4-way and group key handshakes.
pub struct Handshake<'a> {
    cfg: Config<'a>,
    state: State,
    kdf: Kdf,
    mic_algo: Option<MicAlgo>,
    mic_len: usize,
    tk_len: usize,
    pmk: [u8; PMK_LEN],
    snonce: [u8; NONCE_LEN],
    anonce: [u8; NONCE_LEN],
    ptk: Option<Ptk>,
    /// The highest replay counter accepted so far. `None` before message 1,
    /// because zero is a legal counter and so cannot double as "none yet".
    last_replay: Option<u64>,
    /// Scratch for unwrapping Key Data. Held in the struct rather than on the
    /// stack of the handler so that the parsed KDEs can borrow from it.
    key_data: [u8; MAX_KEY_DATA_LEN],
    key_data_len: usize,
    gtk: [u8; MAX_GTK_LEN],
    gtk_len: usize,
    gtk_id: u8,
}

impl core::fmt::Debug for Handshake<'_> {
    /// Prints the state and never the key material — same reasoning as
    /// [`Ptk`]'s: a supplicant logs its state machine, and a `Debug` that
    /// includes the PMK puts the network's password in the log file.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Handshake")
            .field("state", &self.state)
            .field("last_replay", &self.last_replay)
            .field("gtk_len", &self.gtk_len)
            .finish_non_exhaustive()
    }
}

impl<'a> Handshake<'a> {
    /// Begin a handshake.
    ///
    /// `snonce` is the station's nonce for this handshake and **must be
    /// freshly random**: it is the only thing the station contributes to the
    /// PTK, so reusing one across associations with the same AP reproduces
    /// the same PTK and hands an eavesdropper both directions of traffic.
    /// This module cannot generate it — see the module docs.
    ///
    /// Returns `None` if the AKM or the pairwise cipher is not one this stack
    /// implements, which is a decision the caller should have made before
    /// associating but which is cheap to re-check here.
    #[must_use]
    pub fn new(cfg: Config<'a>, pmk: &[u8; PMK_LEN], snonce: [u8; NONCE_LEN]) -> Option<Self> {
        let kdf = kdf::kdf_for_akm(cfg.akm)?;
        let mic_len = eapol::mic_len_for_akm(cfg.akm)?;
        let tk_len = rsn::tk_len(cfg.pairwise)?;
        Some(Self {
            cfg,
            state: State::AwaitingM1,
            kdf,
            // Not known until a frame arrives: the algorithm comes from the
            // Key Descriptor Version, which is a property of the frame rather
            // than of the negotiated suite.
            mic_algo: None,
            mic_len,
            tk_len,
            pmk: *pmk,
            snonce,
            anonce: [0u8; NONCE_LEN],
            ptk: None,
            last_replay: None,
            key_data: [0u8; MAX_KEY_DATA_LEN],
            key_data_len: 0,
            gtk: [0u8; MAX_GTK_LEN],
            gtk_len: 0,
            gtk_id: 0,
        })
    }

    /// True once message 4 has been sent and the keys are installed.
    #[must_use]
    pub fn is_established(&self) -> bool {
        self.state == State::Established
    }

    /// The pairwise temporal key, once [`Outcome::Complete`] has been
    /// returned for the 4-way handshake. Empty before that.
    #[must_use]
    pub fn tk(&self) -> &[u8] {
        match (&self.ptk, self.state) {
            (Some(ptk), State::Established) => ptk.tk(),
            _ => &[],
        }
    }

    /// The group temporal key and its key ID, once one has been received.
    #[must_use]
    pub fn gtk(&self) -> Option<(u8, &[u8])> {
        let key = self.gtk.get(..self.gtk_len)?;
        if key.is_empty() {
            return None;
        }
        Some((self.gtk_id, key))
    }

    /// Feed one received EAPOL frame — the whole frame including the four
    /// octet EAPOL header, as it came off the wire after the LLC/SNAP header
    /// was removed.
    ///
    /// On success `out` holds the reply to transmit; see [`Outcome`] for what
    /// else the caller must do. `out` should be at least 200 octets for the
    /// replies this module produces.
    ///
    /// # Errors
    ///
    /// See [`Error`]. Every rejection leaves the state machine unchanged, so
    /// a rejected frame cannot be used to knock a working link out of its
    /// established state — which matters, because a rejected frame is by
    /// definition one an attacker may have sent.
    pub fn on_eapol(&mut self, eapol_frame: &[u8], out: &mut [u8]) -> Result<Outcome, Error> {
        let body = eapol::body(eapol_frame).ok_or(Error::Malformed)?;
        let key = eapol::KeyFrame::parse(body, self.mic_len).ok_or(Error::Malformed)?;
        let message = key.message().ok_or(Error::Unexpected)?;

        // The MIC covers the frame from its first octet, so verifying one needs
        // the frame and not the body — but it must be the frame the *sender*
        // hashed, which is the header plus exactly the body length the header
        // declares. An EAPOL frame rides inside an 802.11 or Ethernet data
        // frame and is padded to that frame's minimum length, so `eapol_frame`
        // routinely has octets on the end that the sender never hashed.
        // Trimming here rather than at each check keeps the two verifiers from
        // disagreeing about where the frame ends.
        let frame_len = eapol::HEADER_LEN
            .checked_add(body.len())
            .ok_or(Error::Malformed)?;
        let frame = eapol_frame.get(..frame_len).ok_or(Error::Malformed)?;

        // The MIC algorithm is dictated by the Key Descriptor Version rather
        // than by the AKM, and version 1 (HMAC-MD5 over RC4-wrapped key data)
        // is refused outright: accepting it would let an attacker who can
        // edit one field talk a WPA2 station down onto WPA1 cryptography.
        let algo = kdf::mic_algo_for_descriptor_version(key.descriptor_version())
            .ok_or(Error::Unsupported)?;

        match message {
            Message::PairwiseM1 => self.on_m1(&key, algo, out),
            Message::PairwiseM3 => self.on_m3(&key, frame, algo, out),
            Message::GroupM1 => self.on_group_m1(&key, frame, algo, out),
            // Messages 2 and 4 and group message 2 travel the other way. A
            // station receiving one is either talking to itself or being
            // probed; either way it is not a message to act on.
            Message::PairwiseM2 | Message::PairwiseM4 | Message::GroupM2 => Err(Error::Unexpected),
        }
    }

    /// Message 1: the ANonce arrives, the PTK is derived, message 2 goes out.
    ///
    /// Message 1 has no MIC — there is nothing to verify it with, since the
    /// PTK does not exist until it has been received. That is not a flaw: an
    /// attacker who forges a message 1 only causes the station to derive a
    /// PTK the AP will not agree with, and the handshake then fails at
    /// message 3. What it does mean is that message 1 must never *replace* an
    /// in-progress handshake's state, or a forged one could reset a station
    /// that was about to succeed.
    fn on_m1(
        &mut self,
        key: &eapol::KeyFrame<'_>,
        algo: MicAlgo,
        out: &mut [u8],
    ) -> Result<Outcome, Error> {
        if self.state != State::AwaitingM1 {
            return Err(Error::WrongState);
        }
        self.check_replay(key.replay_counter)?;

        let ptk = kdf::derive_ptk(
            self.kdf,
            &self.pmk,
            &self.cfg.bssid,
            &self.cfg.sta,
            &key.nonce,
            &self.snonce,
            self.tk_len,
        )
        .ok_or(Error::Unsupported)?;

        // Everything below this point mutates `self`, and nothing above it
        // can fail — so a rejected message 1 leaves the machine untouched.
        let len = self.write_reply(
            key.replay_counter,
            key_info::PAIRWISE | key_info::KEY_MIC,
            self.snonce,
            self.cfg.sta_rsn_element,
            key.descriptor_version(),
            algo,
            &ptk,
            out,
        )?;

        self.anonce = key.nonce;
        self.ptk = Some(ptk);
        self.mic_algo = Some(algo);
        self.last_replay = Some(key.replay_counter);
        self.state = State::AwaitingM3;
        Ok(Outcome::Reply { len })
    }

    /// Message 3: verify, check for a downgrade, take the GTK, install.
    ///
    /// `frame` is the whole EAPOL frame `key` was parsed out of, trimmed to the
    /// length its header declares — see [`Supplicant::on_eapol`].
    fn on_m3(
        &mut self,
        key: &eapol::KeyFrame<'_>,
        frame: &[u8],
        algo: MicAlgo,
        out: &mut [u8],
    ) -> Result<Outcome, Error> {
        if self.state == State::AwaitingM1 {
            return Err(Error::WrongState);
        }
        self.check_replay(key.replay_counter)?;

        let ptk = self.ptk.as_ref().ok_or(Error::WrongState)?;
        // The MIC first, and nothing from the frame is believed until it
        // passes: the key data that follows is attacker-supplied until this
        // check succeeds, and it is what carries the GTK.
        if !kdf::verify_mic(algo, &ptk.kck, frame, self.mic_len) {
            return Err(Error::BadMic);
        }
        // A different ANonce is a different handshake, and the PTK we hold
        // was derived from the first one. Re-deriving here would let an
        // attacker who can replay message 1 splice two handshakes together.
        if key.nonce != self.anonce {
            return Err(Error::AnonceChanged);
        }

        let kek = ptk.kek;
        let len = decode_key_data(key, &kek, &mut self.key_data)?;
        self.key_data_len = len;
        let data = self.key_data.get(..len).ok_or(Error::BadKeyData)?;

        // The downgrade check the handshake exists for. The beacon is
        // unauthenticated and an attacker can rewrite it; message 3 is
        // authenticated, so if the two disagree the beacon was edited.
        let advertised = KeyData::new(data)
            .find_element(ie::id::RSN)
            .ok_or(Error::RsnMismatch)?;
        if advertised != self.cfg.ap_rsn_element {
            return Err(Error::RsnMismatch);
        }

        let (gtk_id, gtk) = KeyData::new(data).find_gtk().ok_or(Error::NoGroupKey)?;
        if gtk.is_empty() || gtk.len() > MAX_GTK_LEN {
            return Err(Error::BadKeyData);
        }

        // Message 4 is unconditional: a retransmitted message 3 means the AP
        // did not hear the first message 4, and answering is the only way the
        // link recovers. What is conditional is everything else.
        let reply_len = self.write_reply(
            key.replay_counter,
            key_info::PAIRWISE | key_info::KEY_MIC | key_info::SECURE,
            [0u8; NONCE_LEN],
            &[],
            key.descriptor_version(),
            algo,
            ptk,
            out,
        )?;
        self.last_replay = Some(key.replay_counter);

        if self.state == State::Established {
            // KRACK. The keys are already installed; installing them again
            // would reset the CCMP packet number and replay the nonce.
            return Ok(Outcome::Retransmission { len: reply_len });
        }

        self.gtk_id = gtk_id;
        self.gtk_len = gtk.len();
        self.gtk
            .get_mut(..gtk.len())
            .ok_or(Error::BadKeyData)?
            .copy_from_slice(gtk);
        self.state = State::Established;
        Ok(Outcome::Complete { len: reply_len })
    }

    /// Group message 1: a new GTK, outside a 4-way handshake.
    ///
    /// Same shape as message 3 minus the pairwise parts. The replay counter
    /// is the entire defence against a group key reinstallation, which is the
    /// second half of KRACK: unlike message 3 there is no "retransmission"
    /// case to distinguish, because every legitimate group message 1 carries
    /// a fresh counter and a replayed one is simply refused.
    fn on_group_m1(
        &mut self,
        key: &eapol::KeyFrame<'_>,
        frame: &[u8],
        algo: MicAlgo,
        out: &mut [u8],
    ) -> Result<Outcome, Error> {
        if self.state != State::Established {
            return Err(Error::WrongState);
        }
        self.check_replay(key.replay_counter)?;

        let ptk = self.ptk.as_ref().ok_or(Error::WrongState)?;
        if !kdf::verify_mic(algo, &ptk.kck, frame, self.mic_len) {
            return Err(Error::BadMic);
        }

        let kek = ptk.kek;
        let len = decode_key_data(key, &kek, &mut self.key_data)?;
        self.key_data_len = len;
        let data = self.key_data.get(..len).ok_or(Error::BadKeyData)?;
        let (gtk_id, gtk) = KeyData::new(data).find_gtk().ok_or(Error::NoGroupKey)?;
        if gtk.is_empty() || gtk.len() > MAX_GTK_LEN {
            return Err(Error::BadKeyData);
        }

        let reply_len = self.write_reply(
            key.replay_counter,
            key_info::KEY_MIC | key_info::SECURE,
            [0u8; NONCE_LEN],
            &[],
            key.descriptor_version(),
            algo,
            ptk,
            out,
        )?;

        self.last_replay = Some(key.replay_counter);
        self.gtk_id = gtk_id;
        self.gtk_len = gtk.len();
        self.gtk
            .get_mut(..gtk.len())
            .ok_or(Error::BadKeyData)?
            .copy_from_slice(gtk);
        Ok(Outcome::Complete { len: reply_len })
    }

    /// A counter must be strictly greater than the last one accepted.
    ///
    /// Strictly, not "different": accepting a lower counter is accepting a
    /// recording, and accepting an equal one is accepting the same frame
    /// twice. The one exception in the standard — a reply echoing the counter
    /// it answers — does not apply here, because a station never *receives*
    /// a reply.
    fn check_replay(&self, counter: u64) -> Result<(), Error> {
        match self.last_replay {
            Some(last) if counter <= last => Err(Error::Replay),
            _ => Ok(()),
        }
    }

    /// Build one of the three replies this station sends, MIC included.
    #[allow(clippy::too_many_arguments)] // Every argument is a distinct field
    // of the frame being built; bundling them into a struct would only move
    // the same list one line up, and this is a private helper with three
    // call sites in this file.
    fn write_reply(
        &self,
        replay_counter: u64,
        flags: u16,
        nonce: [u8; NONCE_LEN],
        key_data: &[u8],
        descriptor_version: u8,
        algo: MicAlgo,
        ptk: &Ptk,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let fields = KeyFrameFields {
            descriptor_type: eapol::descriptor_type::RSN,
            key_info: u16::from(descriptor_version) | flags,
            // Zero in every RSN message a station sends. The field describes
            // the key the *frame* conveys, and a station conveys none.
            key_len: 0,
            replay_counter,
            nonce,
            iv: [0u8; eapol::IV_LEN],
            rsc: [0u8; eapol::RSC_LEN],
            key_data,
        };
        let len = eapol::write(out, eapol::version::V2, &fields, self.mic_len)
            .ok_or(Error::OutputTooSmall)?;

        // The MIC covers the frame with its own field zeroed, which is the
        // state `eapol::write` leaves it in — so this order is required, not
        // merely convenient.
        let mut mic = [0u8; eapol::MIC_LEN_SUITE_B_192];
        let tag = mic.get_mut(..self.mic_len).ok_or(Error::Unsupported)?;
        let frame = out.get(..len).ok_or(Error::OutputTooSmall)?;
        kdf::compute_mic(algo, &ptk.kck, frame, self.mic_len, tag).ok_or(Error::Unsupported)?;
        let tag_copy = mic;
        let written = out.get_mut(..len).ok_or(Error::OutputTooSmall)?;
        eapol::set_mic(
            written,
            tag_copy.get(..self.mic_len).ok_or(Error::Unsupported)?,
        )
        .ok_or(Error::OutputTooSmall)?;
        Ok(len)
    }
}

// A note on how a received MIC is checked, kept because this module used to do
// it differently and the difference was a bug.
//
// There used to be a private `verify_frame_mic` here that rebuilt the frame
// from the parsed `KeyFrame` and hashed the reconstruction. Its stated reason
// was that "anything we failed to parse is not in what we hash", so a frame
// whose fields we did not fully understand could not pass. That reason cannot
// be satisfied and should not be wanted:
//
//   * It cannot be satisfied, because the MIC is *defined* over the octets the
//     sender put on the wire. Every octet in the hashed range has to be hashed
//     as it arrived, whether this crate has a field for it or not. The rebuild
//     had no field for two of them — the EAPOL version octet at frame offset 0,
//     and the eight reserved octets after the Key RSC — so it substituted
//     version 2 and eight zeroes. An access point that sends version 1 or 3,
//     which `eapol::version`'s own doc comment records as commonplace, had
//     every MIC rejected with a correct passphrase, and the failure is
//     indistinguishable from a wrong one.
//
//   * It should not be wanted, because the property it was reaching for is
//     already held, and held by the MIC itself. An attacker cannot smuggle
//     octets past us by putting them somewhere we do not parse: changing any
//     octet in the frame changes the MIC, and computing a new one needs the
//     KCK. Hashing the frame as received is what makes that true.
//
// So the check is `kdf::verify_mic` over the received frame, which hashes it in
// three pieces — everything before the MIC field, `mic_len` zeroes, everything
// after — and is therefore correct for any version octet and any reserved
// octets. The one thing the caller must get right is where the frame *ends*,
// which `on_eapol` settles once by trimming to the header's declared length.
//
// Reported by lane A, whose authenticator hit the same frame/body distinction
// from the other side; see design-decisions.md §804.

/// Copy the Key Data into `out`, unwrapping it first if it is encrypted.
///
/// Returns the number of octets written.
fn decode_key_data(
    key: &eapol::KeyFrame<'_>,
    kek: &[u8; kdf::KEK_LEN],
    out: &mut [u8; MAX_KEY_DATA_LEN],
) -> Result<usize, Error> {
    if !key.key_data_is_encrypted() {
        // Legal only before the pairwise key exists, which for a station
        // means message 1 — and message 1 carries no key data worth reading.
        // Accepting an unencrypted message 3 would accept a GTK that anyone
        // on the channel could have written.
        return Err(Error::BadKeyData);
    }
    let cipher = aes::Aes::new(kek).map_err(|_| Error::Unsupported)?;
    let plain_len = aes::keywrap::unwrapped_len(key.key_data.len()).ok_or(Error::BadKeyData)?;
    if plain_len > MAX_KEY_DATA_LEN {
        return Err(Error::BadKeyData);
    }
    let n = aes::keywrap::unwrap(&cipher, out, key.key_data).map_err(|_| Error::BadKeyData)?;
    Ok(n)
}

// ---------------------------------------------------------------------------
// Scanning and joining — the frames exchanged before the handshake
// ---------------------------------------------------------------------------

/// What a beacon or probe response says about a network, reduced to the
/// fields a decision to join is actually made from.
// Not `Copy`: `Rsn` is not, because it holds counted sub-slices that are
// cheaper to hand around by reference than to duplicate. A `Candidate` is
// produced once per received beacon and matched on in place, so the copy
// would never be wanted anyway.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    /// The BSSID, which for an infrastructure BSS is also the AP's MAC.
    pub bssid: MacAddr,
    /// The SSID exactly as advertised — possibly empty, for a hidden network,
    /// and never assumed to be UTF-8.
    pub ssid: &'a [u8],
    /// The operating channel, if the frame carried a DS Parameter Set.
    pub channel: Option<u8>,
    /// The beacon interval in TUs.
    pub beacon_interval_tu: u16,
    /// The Privacy bit: the BSS requires some form of encryption.
    pub privacy: bool,
    /// The RSN element body, if present. **Keep this**: message 3 must match
    /// it byte for byte, and comparing against a re-serialised copy would
    /// compare our parser's opinion rather than the AP's bytes.
    pub rsn_element: Option<&'a [u8]>,
    /// The parsed form of the same element.
    pub rsn: Option<Rsn<'a>>,
}

impl Candidate<'_> {
    /// True if this network is protected by an RSN whose AKM this stack can
    /// authenticate with.
    ///
    /// A BSS with Privacy set but no RSN element is WEP, which this stack
    /// does not implement and should not silently treat as open.
    #[must_use]
    pub fn is_joinable(&self) -> bool {
        match &self.rsn {
            Some(r) => r.has_supported_akm(),
            None => !self.privacy,
        }
    }
}

/// Read a received beacon or probe response into a [`Candidate`].
///
/// Returns `None` for anything that is not one of those two, or that is
/// malformed. `frame` is a complete 802.11 frame with its MAC header, without
/// the FCS.
#[must_use]
pub fn scan(frame_bytes: &[u8]) -> Option<Candidate<'_>> {
    let parsed = frame::Frame::parse(frame_bytes)?;
    if parsed.header.fc.frame_type() != FrameType::Management {
        return None;
    }
    let subtype = parsed.header.fc.subtype();
    if subtype != mgmt_subtype::BEACON && subtype != mgmt_subtype::PROBE_RESP {
        return None;
    }
    let body = mgmt::Body::parse(subtype, parsed.body)?;
    let mgmt::Body::Beacon(beacon) = body else {
        return None;
    };
    let elements = beacon.elements;
    let rsn_element = ie::Elements::find_id(elements, ie::id::RSN).map(|e| e.data);
    Some(Candidate {
        bssid: parsed.header.mgmt_bssid()?,
        ssid: ie::ssid(elements).unwrap_or(&[]),
        channel: ie::channel(elements),
        beacon_interval_tu: beacon.beacon_interval,
        privacy: beacon.privacy(),
        rsn_element,
        rsn: rsn_element.and_then(Rsn::parse),
    })
}

/// Build an Open System authentication request.
///
/// Open System "authentication" authenticates nobody — it is a two-frame
/// formality that predates RSN and survives because the state machine in the
/// standard still requires it. The real authentication is the 4-way
/// handshake.
#[must_use]
pub fn write_auth_request(
    out: &mut [u8],
    sta: &MacAddr,
    bssid: &MacAddr,
    sequence: u16,
) -> Option<usize> {
    let mut off = write_mgmt_header(out, mgmt_subtype::AUTH, sta, bssid, sequence)?;
    mgmt::write_auth(out, &mut off, mgmt::auth_alg::OPEN_SYSTEM, 1, 0)?;
    Some(off)
}

/// Build an association request carrying the station's SSID, rates and RSN
/// element.
///
/// `rsn_element` is the element *body*; it is written under
/// [`ie::id::RSN`] and must be the same bytes given to [`Config`] as
/// `sta_rsn_element`, because the AP checks message 2's copy against this
/// one.
#[must_use]
pub fn write_assoc_request(
    out: &mut [u8],
    sta: &MacAddr,
    bssid: &MacAddr,
    ssid: &[u8],
    supported_rates: &[u8],
    rsn_element: &[u8],
    sequence: u16,
) -> Option<usize> {
    let mut off = write_mgmt_header(out, mgmt_subtype::ASSOC_REQ, sta, bssid, sequence)?;
    // ESS, and Privacy because we are asking to join a protected BSS. The
    // listen interval is in beacon intervals; 10 is the usual default and
    // only matters for power save.
    let capability = mgmt::capability::ESS | mgmt::capability::PRIVACY;
    mgmt::write_assoc_req(out, &mut off, capability, 10)?;
    ie::write_ssid(out, &mut off, ssid)?;
    ie::write_element(out, &mut off, ie::id::SUPPORTED_RATES, supported_rates)?;
    ie::write_element(out, &mut off, ie::id::RSN, rsn_element)?;
    Some(off)
}

/// The MAC header shared by every management frame a station sends to its AP:
/// to the AP, from us, in its BSS.
fn write_mgmt_header(
    out: &mut [u8],
    subtype: u8,
    sta: &MacAddr,
    bssid: &MacAddr,
    sequence: u16,
) -> Option<usize> {
    let header = MacHeader {
        fc: FrameControl::new(FrameType::Management, subtype),
        duration_id: 0,
        addr1: *bssid,
        addr2: Some(*sta),
        addr3: Some(*bssid),
        seq_ctrl: Some(SeqCtrl::new(sequence, 0)),
        addr4: None,
        qos_ctrl: None,
        ht_ctrl: None,
        header_len: 24,
    };
    header.write(out)
}

// ---------------------------------------------------------------------------
// The data path, once the keys are installed
// ---------------------------------------------------------------------------

/// Wrap an Ethernet II frame as an 802.11 data frame addressed to the AP.
///
/// `protected` sets the Protected Frame bit, which says the body has been
/// encrypted. This function does *not* encrypt — CCMP lives in the driver,
/// usually in hardware — so pass `false` until the key is installed and
/// `true` afterwards. Getting this backwards sends plaintext with a bit
/// claiming otherwise, which the AP will discard rather than misread.
#[must_use]
pub fn encapsulate(
    out: &mut [u8],
    sta: &MacAddr,
    bssid: &MacAddr,
    ethernet: &[u8],
    sequence: u16,
    protected: bool,
) -> Option<usize> {
    // Address 3 of a to-DS frame is the frame's *final* destination, which is
    // the Ethernet destination — not the AP. Read it straight off the front of
    // the Ethernet header rather than re-deriving it, so a zero-payload frame
    // (legal, and shorter than the SNAP header this would otherwise need room
    // for) is encapsulated like any other.
    let mut dst: MacAddr = [0u8; 6];
    dst.copy_from_slice(ethernet.get(..6)?);

    let header = MacHeader {
        fc: FrameControl::new(FrameType::Data, frame::data_subtype::DATA)
            .with_to_ds(true)
            .with_protected(protected),
        duration_id: 0,
        // To the DS: Address 1 is the AP, Address 3 the final destination.
        addr1: *bssid,
        addr2: Some(*sta),
        addr3: Some(dst),
        seq_ctrl: Some(SeqCtrl::new(sequence, 0)),
        addr4: None,
        qos_ctrl: None,
        ht_ctrl: None,
        header_len: 24,
    };
    let mut off = header.write(out)?;
    let (body_len, _, _) = llc::from_ethernet(out.get_mut(off..)?, ethernet)?;
    off = off.checked_add(body_len)?;
    Some(off)
}

/// Unwrap a received 802.11 data frame into an Ethernet II frame.
///
/// Returns `None` for anything that is not an unfragmented SNAP-encapsulated
/// data frame. The Protected bit is *not* checked here: by the time a frame
/// reaches this function the driver has already decrypted it, and a frame
/// that should have been protected but was not is a policy question the
/// caller answers.
#[must_use]
pub fn decapsulate(out: &mut [u8], frame_bytes: &[u8]) -> Option<usize> {
    let parsed = frame::Frame::parse(frame_bytes)?;
    if parsed.header.fc.frame_type() != FrameType::Data {
        return None;
    }
    let roles = parsed.header.data_addr_roles()?;
    llc::to_ethernet(out, &roles.dst, &roles.src, parsed.body)
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
    const ANONCE: [u8; NONCE_LEN] = [0xA0; NONCE_LEN];
    const SNONCE: [u8; NONCE_LEN] = [0x50; NONCE_LEN];
    const PMK: [u8; PMK_LEN] = [0x0B; PMK_LEN];
    const GTK: [u8; 16] = [0x67; 16];

    /// The RSN element body a WPA2-PSK/CCMP AP advertises.
    const RSN_ELEMENT: [u8; 20] = [
        0x01, 0x00, // version 1
        0x00, 0x0F, 0xAC, 0x04, // group cipher CCMP
        0x01, 0x00, 0x00, 0x0F, 0xAC, 0x04, // one pairwise: CCMP
        0x01, 0x00, 0x00, 0x0F, 0xAC, 0x02, // one AKM: PSK
        0x00, 0x00, // capabilities
    ];

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

    fn handshake() -> Handshake<'static> {
        Handshake::new(config(), &PMK, SNONCE).expect("WPA2-PSK/CCMP is supported")
    }

    /// The PTK both sides of the test agree on, derived the same way the
    /// module does.
    fn ptk() -> Ptk {
        kdf::derive_ptk(Kdf::Sha1, &PMK, &AP, &STA, &ANONCE, &SNONCE, 16)
            .expect("a 16-octet TK is in range")
    }

    /// Build one authenticator-to-station frame, MIC and key wrap included.
    fn ap_frame(
        flags: u16,
        replay: u64,
        nonce: [u8; NONCE_LEN],
        key_data: &[u8],
        encrypt: bool,
    ) -> ([u8; 512], usize) {
        let ptk = ptk();
        let mut wrapped = [0u8; MAX_KEY_DATA_LEN];
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
        let fields = KeyFrameFields {
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
        (out, n)
    }

    fn m1(replay: u64) -> ([u8; 512], usize) {
        ap_frame(
            key_info::PAIRWISE | key_info::KEY_ACK,
            replay,
            ANONCE,
            &[],
            false,
        )
    }

    /// Message 3's key data: the AP's RSN element as a plain IE, plus the GTK
    /// in a KDE, exactly as hostapd builds it.
    fn m3_key_data(gtk: &[u8], key_id: u8) -> ([u8; 128], usize) {
        let mut buf = [0u8; 128];
        let mut off = 0usize;
        buf[off] = ie::id::RSN;
        buf[off + 1] = RSN_ELEMENT.len() as u8;
        buf[off + 2..off + 2 + RSN_ELEMENT.len()].copy_from_slice(&RSN_ELEMENT);
        off += 2 + RSN_ELEMENT.len();

        buf[off] = 221; // vendor-specific
        buf[off + 1] = (4 + 2 + gtk.len()) as u8;
        buf[off + 2..off + 5].copy_from_slice(&rsn::IEEE_OUI);
        buf[off + 5] = kdf::kde::GTK;
        buf[off + 6] = key_id & 0x03;
        buf[off + 7] = 0; // reserved
        buf[off + 8..off + 8 + gtk.len()].copy_from_slice(gtk);
        off += 8 + gtk.len();

        // Key wrap needs a multiple of eight octets; hostapd pads with a
        // single 0xDD followed by zeroes, which the KDE parser stops at.
        while !off.is_multiple_of(8) {
            buf[off] = 0;
            off += 1;
        }
        (buf, off)
    }

    fn m3(replay: u64, nonce: [u8; NONCE_LEN], gtk: &[u8], key_id: u8) -> ([u8; 512], usize) {
        let (data, n) = m3_key_data(gtk, key_id);
        ap_frame(
            key_info::PAIRWISE | key_info::KEY_ACK | key_info::KEY_MIC | key_info::SECURE,
            replay,
            nonce,
            &data[..n],
            true,
        )
    }

    fn group_m1(replay: u64, gtk: &[u8], key_id: u8) -> ([u8; 512], usize) {
        let mut buf = [0u8; 128];
        let mut off = 0usize;
        buf[off] = 221;
        buf[off + 1] = (4 + 2 + gtk.len()) as u8;
        buf[off + 2..off + 5].copy_from_slice(&rsn::IEEE_OUI);
        buf[off + 5] = kdf::kde::GTK;
        buf[off + 6] = key_id & 0x03;
        buf[off + 7] = 0;
        buf[off + 8..off + 8 + gtk.len()].copy_from_slice(gtk);
        off += 8 + gtk.len();
        while !off.is_multiple_of(8) {
            buf[off] = 0;
            off += 1;
        }
        ap_frame(
            key_info::KEY_ACK | key_info::KEY_MIC | key_info::SECURE,
            replay,
            [0u8; NONCE_LEN],
            &buf[..off],
            true,
        )
    }

    /// Drive a whole successful handshake and return the machine plus the two
    /// replies it produced.
    fn run_handshake() -> (Handshake<'static>, [u8; 256], usize, [u8; 256], usize) {
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out2 = [0u8; 256];
        let o1 = hs
            .on_eapol(&f1[..n1], &mut out2)
            .expect("message 1 accepted");
        let len2 = o1.len();

        let (f3, n3) = m3(2, ANONCE, &GTK, 1);
        let mut out4 = [0u8; 256];
        let o3 = hs
            .on_eapol(&f3[..n3], &mut out4)
            .expect("message 3 accepted");
        assert!(o3.installs_keys(), "the first message 3 installs");
        let len4 = o3.len();
        (hs, out2, len2, out4, len4)
    }

    // -- the happy path -----------------------------------------------------

    #[test]
    fn a_whole_handshake_installs_both_keys() {
        let (hs, _, _, _, _) = run_handshake();
        assert!(hs.is_established());
        assert_eq!(hs.tk(), ptk().tk());
        assert_eq!(hs.gtk(), Some((1, &GTK[..])));
    }

    #[test]
    fn message_two_carries_our_nonce_our_rsn_element_and_a_verifiable_mic() {
        let (_, out2, len2, _, _) = run_handshake();
        let body = eapol::body(&out2[..len2]).expect("a body");
        let kf = eapol::KeyFrame::parse(body, eapol::MIC_LEN_DEFAULT).expect("parses");

        assert_eq!(kf.message(), Some(Message::PairwiseM2));
        assert_eq!(kf.nonce, SNONCE);
        assert_eq!(kf.key_data, &RSN_ELEMENT[..]);
        assert_eq!(
            kf.replay_counter, 1,
            "a reply echoes the counter it answers"
        );
        assert!(!kf.wants_reply());

        // The AP's check: recompute the MIC over the frame with its field
        // zeroed and compare.
        assert!(kdf::verify_mic(
            MicAlgo::HmacSha1,
            &ptk().kck,
            &out2[..len2],
            eapol::MIC_LEN_DEFAULT
        ));
    }

    #[test]
    fn message_four_is_empty_secure_and_echoes_the_third_counter() {
        let (_, _, _, out4, len4) = run_handshake();
        let body = eapol::body(&out4[..len4]).expect("a body");
        let kf = eapol::KeyFrame::parse(body, eapol::MIC_LEN_DEFAULT).expect("parses");

        assert_eq!(kf.message(), Some(Message::PairwiseM4));
        assert!(kf.key_data.is_empty());
        assert_eq!(kf.nonce, [0u8; NONCE_LEN]);
        assert!(kf.is_secure());
        assert_eq!(kf.replay_counter, 2);
        assert!(kdf::verify_mic(
            MicAlgo::HmacSha1,
            &ptk().kck,
            &out4[..len4],
            eapol::MIC_LEN_DEFAULT
        ));
    }

    // -- KRACK --------------------------------------------------------------

    #[test]
    fn a_retransmitted_message_three_is_answered_but_does_not_reinstall_the_key() {
        let (mut hs, _, _, _, _) = run_handshake();

        // The AP did not hear message 4 and asks again with a fresh counter,
        // which is what hostapd actually does on retry.
        let (f3, n3) = m3(3, ANONCE, &GTK, 1);
        let mut out = [0u8; 256];
        let outcome = hs.on_eapol(&f3[..n3], &mut out).expect("answered");

        assert!(
            matches!(outcome, Outcome::Retransmission { .. }),
            "a second message 3 must not authorise a second install: reinstalling \
             resets the CCMP packet number and replays the nonce"
        );
        assert!(!outcome.installs_keys());
        assert!(
            !outcome.is_empty(),
            "message 4 must still be sent, or the link never recovers"
        );

        // And the reply is a real message 4, not a stub.
        let body = eapol::body(&out[..outcome.len()]).expect("a body");
        let kf = eapol::KeyFrame::parse(body, eapol::MIC_LEN_DEFAULT).expect("parses");
        assert_eq!(kf.message(), Some(Message::PairwiseM4));
        assert_eq!(kf.replay_counter, 3);
    }

    #[test]
    fn a_replayed_message_three_is_refused_outright() {
        let (mut hs, _, _, _, _) = run_handshake();
        // Byte-identical to the one already accepted, counter included.
        let (f3, n3) = m3(2, ANONCE, &GTK, 1);
        let mut out = [0u8; 256];
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::Replay));
    }

    #[test]
    fn a_replayed_group_message_cannot_reinstall_the_group_key() {
        let (mut hs, _, _, _, _) = run_handshake();
        let new_gtk = [0x99u8; 16];

        let (g1, n1) = group_m1(3, &new_gtk, 2);
        let mut out = [0u8; 256];
        let outcome = hs
            .on_eapol(&g1[..n1], &mut out)
            .expect("a rekey is accepted");
        assert!(outcome.installs_keys());
        assert_eq!(hs.gtk(), Some((2, &new_gtk[..])));

        // The same frame again: refused on the counter, which is the whole
        // defence against group key reinstallation.
        assert_eq!(hs.on_eapol(&g1[..n1], &mut out), Err(Error::Replay));
        assert_eq!(hs.gtk(), Some((2, &new_gtk[..])));
    }

    #[test]
    fn a_counter_that_goes_backwards_is_refused() {
        let mut hs = handshake();
        let (f1, n1) = m1(5);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        let (f3, n3) = m3(4, ANONCE, &GTK, 1);
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::Replay));
        assert!(!hs.is_established());
    }

    // -- forgery and tampering ---------------------------------------------

    #[test]
    fn a_message_three_with_a_wrong_mic_is_refused_before_its_key_data_is_read() {
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        let (mut f3, n3) = m3(2, ANONCE, &GTK, 1);
        f3[eapol::MIC_OFFSET] ^= 0x01;
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::BadMic));
        assert!(!hs.is_established());
        assert_eq!(
            hs.gtk(),
            None,
            "no key may be taken from an unverified frame"
        );
    }

    #[test]
    fn the_wrong_passphrase_shows_up_as_a_bad_mic_on_message_three() {
        // A different PMK is what a different passphrase produces. Message 1
        // still parses — it has no MIC — and the failure surfaces at message
        // 3, which is exactly what a user sees as "it just doesn't connect".
        let mut hs = Handshake::new(config(), &[0x0C; PMK_LEN], SNONCE).expect("supported");
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out)
            .expect("message 1 has no MIC to fail");

        let (f3, n3) = m3(2, ANONCE, &GTK, 1);
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::BadMic));
    }

    // -- the octets inside the MIC that this crate has no field for ---------
    //
    // The MIC covers the frame from offset 0, which puts two runs of octets
    // inside it that `KeyFrame` does not carry: the EAPOL version at offset 0,
    // and the eight reserved octets after the Key RSC. A verifier that rebuilds
    // the frame from the parsed fields has to invent both, and this module's
    // did — version 2 and eight zeroes — so an AP that sent anything else had
    // every MIC rejected with a correct passphrase. These pin the frame down as
    // *received* instead. See design-decisions.md §804.

    /// Recompute and reinstall the MIC over `frame[..len]`, so a test can edit
    /// an octet the sender would have hashed and still present a valid frame.
    fn remic(frame: &mut [u8], len: usize) {
        let ptk = ptk();
        let mut mic = [0u8; eapol::MIC_LEN_DEFAULT];
        kdf::compute_mic(
            MicAlgo::HmacSha1,
            &ptk.kck,
            &frame[..len],
            eapol::MIC_LEN_DEFAULT,
            &mut mic,
        )
        .expect("MIC computes");
        eapol::set_mic(&mut frame[..len], &mic).expect("MIC fits");
    }

    /// Drive message 1, then hand over a message 3 that `edit` has altered and
    /// which has been re-MICed afterwards.
    fn m3_after_editing(edit: impl FnOnce(&mut [u8], usize)) -> Result<Outcome, Error> {
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        let (mut f3, n3) = m3(2, ANONCE, &GTK, 1);
        edit(&mut f3, n3);
        remic(&mut f3, n3);
        hs.on_eapol(&f3[..n3], &mut out)
    }

    #[test]
    fn an_access_point_that_speaks_eapol_version_one_or_three_still_verifies() {
        // 802.1X requires the version to be ignored on receipt and APs in the
        // field send 1, 2 and 3 interchangeably — but the octet is hashed, so
        // "ignored" cannot mean "replaced with the one we happen to send".
        for version in [eapol::version::V1, eapol::version::V3] {
            let outcome = m3_after_editing(|frame, _| frame[0] = version);
            assert!(
                outcome.as_ref().is_ok_and(|o| o.installs_keys()),
                "version {version} should verify, got {outcome:?}"
            );
        }
    }

    #[test]
    fn nonzero_reserved_octets_are_hashed_as_they_arrived() {
        // Reserved fields are "set to 0 on transmit and ignored on receipt",
        // but a sender that sets them anyway MICed them, so ignoring them on
        // receipt cannot extend to leaving them out of the hash.
        let start = eapol::HEADER_LEN + 1 + 2 + 2 + eapol::REPLAY_COUNTER_LEN + eapol::NONCE_LEN;
        let reserved = start + eapol::IV_LEN + eapol::RSC_LEN;
        assert_eq!(
            reserved + eapol::RESERVED_LEN,
            eapol::MIC_OFFSET,
            "the reserved field is the last thing before the MIC"
        );

        let outcome = m3_after_editing(|frame, _| {
            frame[reserved..reserved + eapol::RESERVED_LEN].fill(0xA5);
        });
        assert!(
            outcome.as_ref().is_ok_and(|o| o.installs_keys()),
            "nonzero reserved octets should verify, got {outcome:?}"
        );
    }

    #[test]
    fn padding_past_the_declared_body_is_not_hashed() {
        // An EAPOL frame rides inside a data frame that is padded to a minimum
        // length, so the buffer handed to `on_eapol` is routinely longer than
        // the frame the sender MICed. The header's body length is what says
        // where the sender stopped.
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        let (mut f3, n3) = m3(2, ANONCE, &GTK, 1);
        let padded = n3 + 26;
        f3[n3..padded].fill(0xFF);

        assert!(
            hs.on_eapol(&f3[..padded], &mut out)
                .expect("padding is not part of the frame")
                .installs_keys()
        );
    }

    #[test]
    fn a_message_three_from_a_different_handshake_is_refused() {
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        // A message 3 whose ANonce differs would have to be MICed under a
        // different PTK to get this far, so build it under the PTK we hold
        // but with the nonce changed — the splice an attacker would attempt.
        let (f3, n3) = m3(2, [0xA1; NONCE_LEN], &GTK, 1);
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::AnonceChanged));
    }

    #[test]
    fn a_downgraded_rsn_element_in_message_three_is_caught() {
        // The station saw CCMP in the beacon. Message 3 — which is
        // authenticated, so this is the copy to believe — says TKIP.
        let mut downgraded = RSN_ELEMENT;
        downgraded[5] = rsn::cipher::TKIP;
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        let (mut data, len) = m3_key_data(&GTK, 1);
        data[2..2 + RSN_ELEMENT.len()].copy_from_slice(&downgraded);
        let (f3, n3) = ap_frame(
            key_info::PAIRWISE | key_info::KEY_ACK | key_info::KEY_MIC | key_info::SECURE,
            2,
            ANONCE,
            &data[..len],
            true,
        );
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::RsnMismatch));
        assert!(!hs.is_established());
    }

    #[test]
    fn an_unencrypted_message_three_is_refused_rather_than_read() {
        // Key data in the clear means anyone on the channel could have
        // written the GTK. It is refused even though its MIC is valid.
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        let (data, len) = m3_key_data(&GTK, 1);
        let (f3, n3) = ap_frame(
            key_info::PAIRWISE | key_info::KEY_ACK | key_info::KEY_MIC | key_info::SECURE,
            2,
            ANONCE,
            &data[..len],
            false,
        );
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::BadKeyData));
    }

    #[test]
    fn a_message_three_whose_wrapped_key_data_was_edited_fails_the_unwrap() {
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        // Flip a bit inside the wrapped blob and re-MIC, so the only thing
        // wrong is the key wrap's own integrity check.
        let (data, len) = m3_key_data(&GTK, 1);
        let ptk = ptk();
        let cipher = aes::Aes::new(&ptk.kek).expect("16-octet KEK");
        let mut wrapped = [0u8; MAX_KEY_DATA_LEN];
        let n = aes::keywrap::wrap(&cipher, &mut wrapped, &data[..len]).expect("wraps");
        wrapped[n / 2] ^= 0x40;

        let fields = KeyFrameFields {
            descriptor_type: eapol::descriptor_type::RSN,
            key_info: 2
                | key_info::PAIRWISE
                | key_info::KEY_ACK
                | key_info::KEY_MIC
                | key_info::SECURE
                | key_info::ENCRYPTED_KEY_DATA,
            key_len: 16,
            replay_counter: 2,
            nonce: ANONCE,
            iv: [0u8; eapol::IV_LEN],
            rsc: [0u8; eapol::RSC_LEN],
            key_data: &wrapped[..n],
        };
        let mut f3 = [0u8; 512];
        let n3 = eapol::write(&mut f3, eapol::version::V2, &fields, eapol::MIC_LEN_DEFAULT)
            .expect("fits");
        let mut mic = [0u8; eapol::MIC_LEN_DEFAULT];
        kdf::compute_mic(
            MicAlgo::HmacSha1,
            &ptk.kck,
            &f3[..n3],
            eapol::MIC_LEN_DEFAULT,
            &mut mic,
        )
        .expect("computes");
        eapol::set_mic(&mut f3[..n3], &mic).expect("fits");

        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::BadKeyData));
        assert_eq!(hs.gtk(), None);
    }

    // -- ordering -----------------------------------------------------------

    #[test]
    fn a_message_three_before_any_message_one_is_refused() {
        let mut hs = handshake();
        let (f3, n3) = m3(1, ANONCE, &GTK, 1);
        let mut out = [0u8; 256];
        assert_eq!(hs.on_eapol(&f3[..n3], &mut out), Err(Error::WrongState));
    }

    #[test]
    fn a_second_message_one_cannot_reset_a_handshake_in_progress() {
        // Message 1 is unauthenticated, so if it could restart the state
        // machine anyone could stall a station indefinitely by injecting one.
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        hs.on_eapol(&f1[..n1], &mut out).expect("accepted");

        let (f1b, n1b) = m1(2);
        assert_eq!(hs.on_eapol(&f1b[..n1b], &mut out), Err(Error::WrongState));

        // And the real message 3 still completes, proving the state survived.
        let (f3, n3) = m3(2, ANONCE, &GTK, 1);
        assert!(
            hs.on_eapol(&f3[..n3], &mut out)
                .expect("accepted")
                .installs_keys()
        );
    }

    #[test]
    fn a_group_rekey_before_the_pairwise_handshake_is_refused() {
        let mut hs = handshake();
        let (g1, n1) = group_m1(1, &GTK, 1);
        let mut out = [0u8; 256];
        assert_eq!(hs.on_eapol(&g1[..n1], &mut out), Err(Error::WrongState));
    }

    #[test]
    fn a_message_the_station_itself_sends_is_not_acted_on() {
        // Message 2 arriving at a station means something is echoing our own
        // traffic back at us.
        let (mut hs, out2, len2, _, _) = run_handshake();
        let mut out = [0u8; 256];
        assert_eq!(hs.on_eapol(&out2[..len2], &mut out), Err(Error::Unexpected));
    }

    #[test]
    fn descriptor_version_one_is_refused_as_a_downgrade() {
        // Version 1 is HMAC-MD5 with RC4-wrapped key data. Accepting it would
        // let one edited octet move a WPA2 station onto WPA1 cryptography.
        let mut hs = handshake();
        let (mut f1, n1) = m1(1);
        // Key Info is a big-endian u16 at body offset 1, so its low octet —
        // which holds the version — is at frame offset 4 + 2.
        f1[6] = (f1[6] & !0x07) | 1;
        let mut out = [0u8; 256];
        assert_eq!(hs.on_eapol(&f1[..n1], &mut out), Err(Error::Unsupported));
    }

    #[test]
    fn a_truncated_frame_is_rejected_not_read_past() {
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut out = [0u8; 256];
        for cut in 0..n1 {
            // Any prefix must be refused, and none may panic.
            assert!(hs.on_eapol(&f1[..cut], &mut out).is_err());
        }
        assert!(hs.on_eapol(&f1[..n1], &mut out).is_ok());
    }

    #[test]
    fn an_output_buffer_too_small_for_the_reply_is_an_error_not_a_truncation() {
        let mut hs = handshake();
        let (f1, n1) = m1(1);
        let mut tiny = [0u8; 32];
        assert_eq!(
            hs.on_eapol(&f1[..n1], &mut tiny),
            Err(Error::OutputTooSmall)
        );
        // And the failure left the machine able to try again properly.
        let mut out = [0u8; 256];
        assert!(hs.on_eapol(&f1[..n1], &mut out).is_ok());
    }

    #[test]
    fn the_debug_impl_does_not_print_key_material() {
        use core::fmt::Write as _;
        let (hs, _, _, _, _) = run_handshake();
        let mut s = String::new();
        write!(s, "{hs:?}").expect("formatting to a String cannot fail");

        // The useful half: the state machine is still debuggable.
        assert!(s.contains("Established"), "state is missing from {s}");

        // The half that matters: no field that carries key material is
        // printed. This is spelled as a name check rather than a byte check
        // on purpose — a byte check passes by luck whenever the key happens
        // not to contain the fixture's bytes, whereas this fails the moment
        // someone adds `.field("pmk", &self.pmk)`, which is the mistake being
        // guarded against.
        for secret in ["pmk", "ptk", "snonce", "anonce", "gtk:", "key_data"] {
            assert!(
                !s.to_ascii_lowercase().contains(secret),
                "Debug leaks `{secret}`: {s}"
            );
        }
    }

    // -- unsupported suites -------------------------------------------------

    #[test]
    fn an_akm_this_stack_cannot_authenticate_is_refused_at_construction() {
        let cfg = Config {
            akm: rsn::akm::FT_DOT1X_SHA384,
            ..config()
        };
        assert!(
            Handshake::new(cfg, &PMK, SNONCE).is_none(),
            "Suite-B 192 needs SHA-384, which this stack does not implement; \
             approximating it with SHA-256 would produce a key that silently \
             never matches"
        );
    }

    #[test]
    fn a_cipher_with_no_defined_key_length_is_refused_at_construction() {
        let cfg = Config {
            pairwise: rsn::Suite::standard(0x7F),
            ..config()
        };
        assert!(Handshake::new(cfg, &PMK, SNONCE).is_none());
    }

    // -- scanning and joining ----------------------------------------------

    /// A beacon with an SSID, a DS parameter set and an RSN element.
    fn beacon() -> ([u8; 256], usize) {
        let mut out = [0u8; 256];
        let header = MacHeader {
            fc: FrameControl::new(FrameType::Management, mgmt_subtype::BEACON),
            duration_id: 0,
            addr1: crate::BROADCAST_MAC,
            addr2: Some(AP),
            addr3: Some(AP),
            seq_ctrl: Some(SeqCtrl::new(7, 0)),
            addr4: None,
            qos_ctrl: None,
            ht_ctrl: None,
            header_len: 24,
        };
        let mut off = header.write(&mut out).expect("header fits");
        mgmt::write_beacon(
            &mut out,
            &mut off,
            0x0123_4567_89AB_CDEF,
            100,
            mgmt::capability::ESS | mgmt::capability::PRIVACY,
        )
        .expect("body fits");
        ie::write_ssid(&mut out, &mut off, b"slateos").expect("ssid fits");
        ie::write_element(&mut out, &mut off, ie::id::DS_PARAMETER_SET, &[6]).expect("ds fits");
        ie::write_element(&mut out, &mut off, ie::id::RSN, &RSN_ELEMENT).expect("rsn fits");
        (out, off)
    }

    #[test]
    fn a_beacon_becomes_a_joinable_candidate() {
        let (buf, n) = beacon();
        let c = scan(&buf[..n]).expect("a beacon parses");
        assert_eq!(c.bssid, AP);
        assert_eq!(c.ssid, b"slateos");
        assert_eq!(c.channel, Some(6));
        assert_eq!(c.beacon_interval_tu, 100);
        assert!(c.privacy);
        assert!(c.is_joinable());
        assert_eq!(c.rsn_element, Some(&RSN_ELEMENT[..]));
        assert!(c.rsn.expect("an RSN element").is_psk());
    }

    #[test]
    fn the_raw_rsn_element_is_kept_so_message_three_can_be_compared_against_it() {
        // The comparison in message 3 is byte-for-byte against what the AP
        // actually sent. Keeping only the parsed form would compare our
        // parser's opinion with itself and detect no downgrade at all.
        let (buf, n) = beacon();
        let c = scan(&buf[..n]).expect("parses");
        let raw = c.rsn_element.expect("present");
        assert_eq!(raw, &RSN_ELEMENT[..]);
        assert!(
            raw.as_ptr() >= buf.as_ptr() && raw.as_ptr() <= buf.as_ptr().wrapping_add(n),
            "the candidate must borrow the beacon's own bytes, not a copy"
        );
    }

    #[test]
    fn a_privacy_bss_with_no_rsn_element_is_wep_and_not_joinable() {
        let mut out = [0u8; 256];
        let header = MacHeader {
            fc: FrameControl::new(FrameType::Management, mgmt_subtype::BEACON),
            duration_id: 0,
            addr1: crate::BROADCAST_MAC,
            addr2: Some(AP),
            addr3: Some(AP),
            seq_ctrl: Some(SeqCtrl::new(1, 0)),
            addr4: None,
            qos_ctrl: None,
            ht_ctrl: None,
            header_len: 24,
        };
        let mut off = header.write(&mut out).expect("fits");
        mgmt::write_beacon(
            &mut out,
            &mut off,
            0,
            100,
            mgmt::capability::ESS | mgmt::capability::PRIVACY,
        )
        .expect("fits");
        ie::write_ssid(&mut out, &mut off, b"old").expect("fits");
        let c = scan(&out[..off]).expect("parses");
        assert!(c.privacy);
        assert!(c.rsn.is_none());
        assert!(!c.is_joinable(), "WEP is not a thing this stack will join");
    }

    #[test]
    fn a_data_frame_is_not_a_scan_result() {
        let mut out = [0u8; 128];
        let n = encapsulate(&mut out, &STA, &AP, &[0u8; 20], 1, false).expect("fits");
        assert!(scan(&out[..n]).is_none());
    }

    #[test]
    fn the_auth_and_assoc_requests_are_addressed_to_the_ap_and_parse_back() {
        let mut out = [0u8; 256];
        let n = write_auth_request(&mut out, &STA, &AP, 1).expect("fits");
        let f = frame::Frame::parse(&out[..n]).expect("parses");
        assert_eq!(f.header.fc.subtype(), mgmt_subtype::AUTH);
        assert_eq!(f.header.addr1, AP);
        assert_eq!(f.header.addr2, Some(STA));
        let mgmt::Body::Auth(a) = mgmt::Body::parse(mgmt_subtype::AUTH, f.body).expect("body")
        else {
            panic!("an auth frame parses as an auth body");
        };
        assert_eq!(a.algorithm, mgmt::auth_alg::OPEN_SYSTEM);
        assert_eq!(a.seq, 1);

        let n = write_assoc_request(
            &mut out,
            &STA,
            &AP,
            b"slateos",
            &[0x82, 0x84],
            &RSN_ELEMENT,
            2,
        )
        .expect("fits");
        let f = frame::Frame::parse(&out[..n]).expect("parses");
        let mgmt::Body::AssocReq(r) =
            mgmt::Body::parse(mgmt_subtype::ASSOC_REQ, f.body).expect("body")
        else {
            panic!("an assoc request parses as one");
        };
        assert_eq!(ie::ssid(r.elements), Some(&b"slateos"[..]));
        assert_eq!(
            ie::Elements::find_id(r.elements, ie::id::RSN).map(|e| e.data),
            Some(&RSN_ELEMENT[..]),
            "the RSN element must go out byte-identical to the one the \
             handshake will later echo in message 2"
        );
    }

    #[test]
    fn an_output_buffer_too_small_for_a_management_frame_is_refused() {
        let mut tiny = [0u8; 16];
        assert!(write_auth_request(&mut tiny, &STA, &AP, 1).is_none());
        assert!(
            write_assoc_request(&mut tiny, &STA, &AP, b"x", &[0x82], &RSN_ELEMENT, 1).is_none()
        );
    }

    // -- the data path ------------------------------------------------------

    #[test]
    fn an_ethernet_frame_survives_a_round_trip_through_the_air() {
        let mut eth = [0u8; 64];
        eth[0..6].copy_from_slice(&[0xFF; 6]); // to broadcast
        eth[6..12].copy_from_slice(&STA);
        eth[12..14].copy_from_slice(&0x0800u16.to_be_bytes()); // IPv4
        for (i, b) in eth[14..].iter_mut().enumerate() {
            *b = i as u8;
        }

        let mut air = [0u8; 128];
        let n = encapsulate(&mut air, &STA, &AP, &eth, 3, true).expect("fits");

        let f = frame::Frame::parse(&air[..n]).expect("parses");
        assert_eq!(f.header.fc.frame_type(), FrameType::Data);
        assert!(f.header.fc.to_ds(), "a station sends towards the DS");
        assert!(!f.header.fc.from_ds());
        assert!(f.header.fc.protected());
        assert_eq!(f.header.addr1, AP, "address 1 is the hop's receiver");
        assert_eq!(
            f.header.addr3,
            Some([0xFFu8; 6]),
            "address 3 is the real destination"
        );

        // An AP relaying it back to us swaps the DS bits round.
        let mut back = [0u8; 128];
        let header = MacHeader {
            fc: FrameControl::new(FrameType::Data, frame::data_subtype::DATA).with_from_ds(true),
            duration_id: 0,
            addr1: STA,
            addr2: Some(AP),
            addr3: Some(STA),
            seq_ctrl: Some(SeqCtrl::new(4, 0)),
            addr4: None,
            qos_ctrl: None,
            ht_ctrl: None,
            header_len: 24,
        };
        let hn = header.write(&mut back).expect("fits");
        back[hn..n].copy_from_slice(&air[24..n]);

        let mut out = [0u8; 128];
        let m = decapsulate(&mut out, &back[..n]).expect("decapsulates");
        assert_eq!(&out[14..m], &eth[14..], "the payload is unchanged");
        assert_eq!(&out[12..14], &0x0800u16.to_be_bytes());
    }

    #[test]
    fn the_protected_bit_reflects_what_the_caller_asked_for() {
        // Before the handshake the bit must be clear; setting it early sends
        // plaintext the AP will drop as if it were corrupt.
        let eth = [0u8; 20];
        let mut air = [0u8; 128];
        let n = encapsulate(&mut air, &STA, &AP, &eth, 1, false).expect("fits");
        assert!(
            !frame::Frame::parse(&air[..n])
                .expect("parses")
                .header
                .fc
                .protected()
        );
        let n = encapsulate(&mut air, &STA, &AP, &eth, 1, true).expect("fits");
        assert!(
            frame::Frame::parse(&air[..n])
                .expect("parses")
                .header
                .fc
                .protected()
        );
    }

    #[test]
    fn a_management_frame_is_not_decapsulated_as_data() {
        let (buf, n) = beacon();
        let mut out = [0u8; 128];
        assert!(decapsulate(&mut out, &buf[..n]).is_none());
    }
}
