//! EAPOL and the EAPOL-Key frame (IEEE 802.1X-2020 §11, IEEE 802.11-2020
//! §12.7.2) — the frames the WPA2/WPA3 4-way handshake is carried in.
//!
//! # This module is the only big-endian one
//!
//! Everything else in this crate is little-endian, because everything else is
//! an 802.11 field. EAPOL is not an 802.11 protocol: it is IEEE 802.1X, which
//! runs over any 802 LAN and uses network byte order like Ethernet and IP.
//! Every multi-octet integer here — the body length, Key Information, Key
//! Length, the replay counter, Key Data Length — is **big-endian**. Reading
//! Key Information little-endian is the classic port-from-802.11 bug: it turns
//! a pairwise message-3 into a group-key message and the handshake fails with
//! no obvious cause.
//!
//! # Where an EAPOL frame lives
//!
//! On the air it is the payload of an 802.11 data frame with an LLC/SNAP
//! header whose EtherType is [`crate::llc::ETHERTYPE_EAPOL`] (`0x888E`). So the
//! path in is: strip the MAC header ([`crate::frame`]), strip SNAP
//! ([`crate::llc`]), and what remains starts at [`Header`].
//!
//! # What is here and what is not
//!
//! This module is *framing only*. It parses and builds the octets; it computes
//! no keys and verifies no MICs. That split is deliberate: framing needs no
//! crypto dependency, is exhaustively testable against fixed vectors, and is
//! what the kernel-side driver needs in order to hand a frame up to the
//! supplicant without understanding it. The PTK derivation, the MIC and the
//! AES key unwrap of the GTK belong with the supplicant's state machine.
//!
//! The one concession to the crypto layer is [`clear_mic`]: the MIC is computed
//! over the whole frame *with the MIC field zeroed*, which is a property of the
//! framing and not of the hash, so the framing layer is where it belongs.
//!
//! # References
//!
//! - IEEE Std 802.1X-2020, §11.3 (EAPOL PDU structure), §11.9 (EAPOL-Key).
//! - IEEE Std 802.11-2020, §12.7.2 (EAPOL-Key frames), §12.7.6 (4-way
//!   handshake), §12.7.7 (group key handshake).

use crate::rsn::akm;

/// The length of the EAPOL header that precedes every packet body.
pub const HEADER_LEN: usize = 4;

/// The length of a Key Nonce (`ANonce`, `SNonce`).
pub const NONCE_LEN: usize = 32;

/// The length of the (obsolete) EAPOL Key IV field.
pub const IV_LEN: usize = 16;

/// The length of the Key RSC field — the group key's starting packet number.
pub const RSC_LEN: usize = 8;

/// The length of the reserved field that follows the Key RSC.
pub const RESERVED_LEN: usize = 8;

/// The length of the Key Replay Counter field.
pub const REPLAY_COUNTER_LEN: usize = 8;

/// The Key MIC length for every AKM in common use (HMAC-SHA1-128 truncated to
/// 128 bits, or AES-128-CMAC — both 16 octets).
pub const MIC_LEN_DEFAULT: usize = 16;

/// The Key MIC length for the Suite B 192-bit AKMs, which use HMAC-SHA-384
/// truncated to 192 bits.
pub const MIC_LEN_SUITE_B_192: usize = 24;

/// EAPOL protocol versions (802.1X-2020 §11.3.1).
///
/// A receiver must accept a version it does not know: 802.1X requires the
/// version to be ignored on receipt, and APs in the field send 1, 2 and 3
/// interchangeably. Only the sender picks one.
pub mod version {
    /// IEEE 802.1X-2001. Still what many supplicants send, for compatibility.
    pub const V1: u8 = 1;
    /// IEEE 802.1X-2004.
    pub const V2: u8 = 2;
    /// IEEE 802.1X-2010 and later.
    pub const V3: u8 = 3;
}

/// EAPOL packet types (802.1X-2020 table 11-3).
pub mod packet_type {
    /// An encapsulated EAP packet — used by 802.1X/enterprise authentication.
    pub const EAP_PACKET: u8 = 0;
    /// EAPOL-Start: the supplicant asking the authenticator to begin.
    pub const START: u8 = 1;
    /// EAPOL-Logoff.
    pub const LOGOFF: u8 = 2;
    /// EAPOL-Key: the 4-way and group key handshakes. The only type WPA-PSK
    /// networks use.
    pub const KEY: u8 = 3;
    /// EAPOL-Encapsulated-ASF-Alert.
    pub const ASF_ALERT: u8 = 4;
    /// EAPOL-MKA (MACsec key agreement).
    pub const MKA: u8 = 5;
    /// EAPOL-Announcement (generic).
    pub const ANNOUNCEMENT_GENERIC: u8 = 6;
    /// EAPOL-Announcement (specific).
    pub const ANNOUNCEMENT_SPECIFIC: u8 = 7;
    /// EAPOL-Announcement-Req.
    pub const ANNOUNCEMENT_REQ: u8 = 8;
}

/// Key Descriptor Type values (802.11-2020 §12.7.2).
pub mod descriptor_type {
    /// The IEEE 802.11 key descriptor — WPA2 and WPA3 (RSN).
    pub const RSN: u8 = 2;
    /// The vendor-specific WPA1 key descriptor. Still seen on old APs.
    pub const WPA1: u8 = 254;
}

/// The Key Information bitfield (802.11-2020 §12.7.2, figure 12-34).
///
/// The whole field is a big-endian `u16`; these are masks over the *host*
/// value after byte-swapping, numbered from the least significant bit.
pub mod key_info {
    /// Key Descriptor Version, bits 0-2. See [`descriptor_version`].
    pub const VERSION_MASK: u16 = 0x0007;
    /// Key Type: set for a pairwise key, clear for a group/SMK key.
    pub const PAIRWISE: u16 = 0x0008;
    /// Key Index, bits 4-5 — used only by the WPA1 group key handshake and
    /// reserved (zero) in RSN.
    pub const KEY_INDEX_MASK: u16 = 0x0030;
    /// The shift needed to bring [`KEY_INDEX_MASK`] down to bit 0.
    pub const KEY_INDEX_SHIFT: u32 = 4;
    /// Install: the receiver should install the key it just derived or was
    /// given. Set in message 3 and in group message 1.
    pub const INSTALL: u16 = 0x0040;
    /// Key ACK: the sender expects a reply. Set on every frame the
    /// authenticator sends, clear on every frame the supplicant sends — which
    /// is the cheapest way to tell "is this frame for me to answer?".
    pub const KEY_ACK: u16 = 0x0080;
    /// Key MIC: the Key MIC field is present and populated.
    pub const KEY_MIC: u16 = 0x0100;
    /// Secure: the pairwise key is installed and the link is protected.
    pub const SECURE: u16 = 0x0200;
    /// Error: a MIC failure report (TKIP Michael countermeasures).
    pub const ERROR: u16 = 0x0400;
    /// Request: the supplicant is asking for a handshake to be started.
    pub const REQUEST: u16 = 0x0800;
    /// Encrypted Key Data: the Key Data field is wrapped and must be
    /// unwrapped (AES key unwrap, or RC4 for descriptor version 1) before it
    /// can be parsed as information elements.
    pub const ENCRYPTED_KEY_DATA: u16 = 0x1000;
    /// SMK Message: part of a PeerKey/SMK handshake, not a 4-way handshake.
    pub const SMK_MESSAGE: u16 = 0x2000;

    /// Key Descriptor Version 1: HMAC-MD5 MIC with RC4-wrapped key data. TKIP
    /// only, and long obsolete.
    pub const VERSION_HMAC_MD5_RC4: u8 = 1;
    /// Key Descriptor Version 2: HMAC-SHA1-128 MIC with AES key wrap. This is
    /// what a WPA2-PSK network uses.
    pub const VERSION_HMAC_SHA1_AES: u8 = 2;
    /// Key Descriptor Version 3: AES-128-CMAC MIC with AES key wrap. Used when
    /// the AKM is one of the SHA-256 suites.
    pub const VERSION_AES_CMAC_AES: u8 = 3;
    /// Key Descriptor Version 0: the MIC field is absent entirely and the Key
    /// Data is protected by an AEAD instead. Used by SAE with hash-to-element,
    /// by FILS and by Suite B.
    pub const VERSION_AEAD: u8 = 0;

    /// The Key Descriptor Version carried in bits 0-2.
    #[must_use]
    pub fn descriptor_version(info: u16) -> u8 {
        // Truncation is impossible: the mask is three bits wide.
        #[allow(clippy::cast_possible_truncation)]
        {
            (info & VERSION_MASK) as u8
        }
    }

    /// The Key Index carried in bits 4-5.
    #[must_use]
    pub fn key_index(info: u16) -> u8 {
        #[allow(clippy::cast_possible_truncation)]
        {
            ((info & KEY_INDEX_MASK) >> KEY_INDEX_SHIFT) as u8
        }
    }
}

/// The length of the Key MIC field for an AKM suite type, or `None` if that
/// AKM uses an AEAD and has no MIC field at all.
///
/// The MIC length is *not* carried in the frame. A receiver must already know
/// the AKM — from the RSN element it accepted during association — before it
/// can find the Key Data Length field, because that field sits immediately
/// after a MIC of a length only the AKM tells you. This is why [`KeyFrame::parse`]
/// takes the MIC length as an argument rather than discovering it.
///
/// Returns `None` for an unknown AKM as well as for an AEAD one; the caller
/// cannot parse either, and conflating them costs nothing here because the
/// caller has already refused to associate with an AKM it does not implement.
#[must_use]
pub fn mic_len_for_akm(akm_type: u8) -> Option<usize> {
    match akm_type {
        akm::DOT1X
        | akm::PSK
        | akm::FT_DOT1X
        | akm::FT_PSK
        | akm::DOT1X_SHA256
        | akm::PSK_SHA256
        | akm::TDLS
        | akm::SAE
        | akm::FT_SAE
        | akm::AP_PEERKEY
        | akm::DOT1X_SUITE_B => Some(MIC_LEN_DEFAULT),
        akm::DOT1X_SUITE_B_192 | akm::FT_DOT1X_SHA384 => Some(MIC_LEN_SUITE_B_192),
        // FILS and OWE derive keys without a 4-way MIC field; so does SAE with
        // hash-to-element when it negotiates an AEAD descriptor.
        _ => None,
    }
}

/// The offset of the Key MIC field from the start of the *EAPOL frame* (that
/// is, including the 4-octet EAPOL header).
///
/// Descriptor type (1) + Key Information (2) + Key Length (2) + Replay Counter
/// (8) + Nonce (32) + IV (16) + RSC (8) + Reserved (8) = 77 octets of body
/// before the MIC.
pub const MIC_OFFSET: usize = HEADER_LEN + 77;

/// The number of body octets that precede the Key MIC field.
const BODY_BEFORE_MIC: usize = MIC_OFFSET - HEADER_LEN;

/// The total EAPOL-Key body length for a frame with `mic_len` octets of MIC and
/// `key_data_len` octets of key data.
#[must_use]
pub fn body_len(mic_len: usize, key_data_len: usize) -> Option<usize> {
    BODY_BEFORE_MIC
        .checked_add(mic_len)?
        .checked_add(2)?
        .checked_add(key_data_len)
}

/// Read a big-endian `u16` at `at`, or `None` if short.
///
/// Named against the grain of the rest of the crate on purpose: seeing `be_`
/// here should prompt "why is this one big-endian?" and the module docs answer.
fn be_u16(buf: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let s: [u8; 2] = buf.get(at..end)?.try_into().ok()?;
    Some(u16::from_be_bytes(s))
}

/// Read a big-endian `u64` at `at`, or `None` if short.
fn be_u64(buf: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    let s: [u8; 8] = buf.get(at..end)?.try_into().ok()?;
    Some(u64::from_be_bytes(s))
}

/// The 4-octet EAPOL header that precedes every packet body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Protocol version. Ignored on receipt, per 802.1X.
    pub version: u8,
    /// One of [`packet_type`].
    pub packet_type: u8,
    /// The length of the body that follows, in octets. **Big-endian on the
    /// wire.**
    pub body_len: u16,
}

impl Header {
    /// Parse the header at the start of `frame`.
    #[must_use]
    pub fn parse(frame: &[u8]) -> Option<Self> {
        Some(Self {
            version: *frame.first()?,
            packet_type: *frame.get(1)?,
            body_len: be_u16(frame, 2)?,
        })
    }

    /// Write the header into the start of `out`, returning `HEADER_LEN`.
    #[must_use]
    pub fn write(&self, out: &mut [u8]) -> Option<usize> {
        let dst = out.get_mut(..HEADER_LEN)?;
        *dst.get_mut(0)? = self.version;
        *dst.get_mut(1)? = self.packet_type;
        dst.get_mut(2..4)?
            .copy_from_slice(&self.body_len.to_be_bytes());
        Some(HEADER_LEN)
    }
}

/// The body of an EAPOL frame — exactly the octets the header says, and no
/// more.
///
/// **Trailing octets are not an error and must be ignored.** An EAPOL frame
/// travels inside an Ethernet or 802.11 data frame, and an Ethernet frame is
/// padded to a 60-octet minimum. A supplicant that treats "the rest of the
/// buffer" as the body will parse the padding as key data on every short
/// handshake message. That is why this returns a slice of the declared length
/// rather than `&frame[HEADER_LEN..]`.
#[must_use]
pub fn body(frame: &[u8]) -> Option<&[u8]> {
    let hdr = Header::parse(frame)?;
    let end = HEADER_LEN.checked_add(usize::from(hdr.body_len))?;
    frame.get(HEADER_LEN..end)
}

/// A parsed EAPOL-Key body.
///
/// The variable-length tail fields borrow from the input; nothing is copied
/// except the fixed-size nonce, IV and RSC, which are small and are far easier
/// to use by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyFrame<'a> {
    /// [`descriptor_type::RSN`] or [`descriptor_type::WPA1`].
    pub descriptor_type: u8,
    /// The Key Information bitfield, host order. See [`key_info`].
    pub key_info: u16,
    /// The length of the temporal key this frame refers to, in octets. 16 for
    /// CCMP-128, 32 for TKIP, and 0 in a message that conveys no key.
    pub key_len: u16,
    /// The replay counter. A supplicant must reject a frame whose counter is
    /// not greater than the last one it accepted (except in message 4 and
    /// group message 2, which echo the counter they answer).
    pub replay_counter: u64,
    /// `ANonce` when the authenticator sent the frame, `SNonce` when the
    /// supplicant did. All-zero in message 4.
    pub nonce: [u8; NONCE_LEN],
    /// The (obsolete) EAPOL Key IV. Zero in everything but WPA1 group keying.
    pub iv: [u8; IV_LEN],
    /// The group key's starting receive sequence counter.
    pub rsc: [u8; RSC_LEN],
    /// The Key MIC, exactly `mic_len` octets. Empty when the AKM is an AEAD
    /// one and the frame therefore has no MIC field.
    pub mic: &'a [u8],
    /// The Key Data field. Information elements when
    /// [`key_info::ENCRYPTED_KEY_DATA`] is clear; an AES-wrapped blob that must
    /// be unwrapped first when it is set.
    pub key_data: &'a [u8],
}

impl<'a> KeyFrame<'a> {
    /// Parse an EAPOL-Key *body* (that is, the output of [`body`]).
    ///
    /// `mic_len` comes from [`mic_len_for_akm`] for the AKM that was
    /// negotiated during association — it is not discoverable from the frame.
    /// Pass `0` for an AEAD AKM, whose frames have no MIC field.
    ///
    /// Returns `None` if the body is short, or if the Key Data Length field
    /// does not match the octets actually present. A Key Data Length that
    /// overruns the frame must be rejected rather than clamped: clamping would
    /// let an attacker truncate the RSN element an AP advertised in message 3,
    /// which is precisely the downgrade the handshake exists to detect.
    #[must_use]
    pub fn parse(body: &'a [u8], mic_len: usize) -> Option<Self> {
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(body.get(13..45)?);
        let mut iv = [0u8; IV_LEN];
        iv.copy_from_slice(body.get(45..61)?);
        let mut rsc = [0u8; RSC_LEN];
        rsc.copy_from_slice(body.get(61..69)?);

        let mic_end = BODY_BEFORE_MIC.checked_add(mic_len)?;
        let mic = body.get(BODY_BEFORE_MIC..mic_end)?;
        let key_data_len = usize::from(be_u16(body, mic_end)?);
        let data_start = mic_end.checked_add(2)?;
        let data_end = data_start.checked_add(key_data_len)?;
        let key_data = body.get(data_start..data_end)?;

        Some(Self {
            descriptor_type: *body.first()?,
            key_info: be_u16(body, 1)?,
            key_len: be_u16(body, 3)?,
            replay_counter: be_u64(body, 5)?,
            nonce,
            iv,
            rsc,
            mic,
            key_data,
        })
    }

    /// The Key Descriptor Version from the Key Information field.
    #[must_use]
    pub fn descriptor_version(&self) -> u8 {
        key_info::descriptor_version(self.key_info)
    }

    /// True if this frame concerns the pairwise key rather than the group key.
    #[must_use]
    pub fn is_pairwise(&self) -> bool {
        (self.key_info & key_info::PAIRWISE) != 0
    }

    /// True if the sender expects a reply — set on every authenticator-to-
    /// supplicant frame and clear on every reply.
    #[must_use]
    pub fn wants_reply(&self) -> bool {
        (self.key_info & key_info::KEY_ACK) != 0
    }

    /// True if the Key MIC field is populated and must be verified.
    #[must_use]
    pub fn has_mic(&self) -> bool {
        (self.key_info & key_info::KEY_MIC) != 0
    }

    /// True if the Key Data field is wrapped and cannot be parsed as
    /// information elements until it has been unwrapped.
    #[must_use]
    pub fn key_data_is_encrypted(&self) -> bool {
        (self.key_info & key_info::ENCRYPTED_KEY_DATA) != 0
    }

    /// True if the sender considers the pairwise key installed.
    #[must_use]
    pub fn is_secure(&self) -> bool {
        (self.key_info & key_info::SECURE) != 0
    }

    /// True if this frame reports a MIC failure (TKIP Michael countermeasures)
    /// rather than advancing a handshake.
    #[must_use]
    pub fn is_error(&self) -> bool {
        (self.key_info & key_info::ERROR) != 0
    }

    /// Which handshake message this is, if it is one. See [`Message`].
    #[must_use]
    pub fn message(&self) -> Option<Message> {
        classify(self.key_info, self.key_data.len())
    }
}

/// A message of the 4-way handshake (802.11-2020 §12.7.6) or of the group key
/// handshake (§12.7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// AP → STA: carries `ANonce`, no MIC. The station derives the PTK from
    /// it.
    PairwiseM1,
    /// STA → AP: carries `SNonce` and the station's RSN element, with a MIC.
    PairwiseM2,
    /// AP → STA: carries `ANonce` again, the AP's RSN element and the wrapped
    /// GTK, with a MIC.
    PairwiseM3,
    /// STA → AP: the acknowledgement. No nonce, no key data.
    PairwiseM4,
    /// AP → STA: a new wrapped GTK, outside a 4-way handshake.
    GroupM1,
    /// STA → AP: the acknowledgement of a new GTK.
    GroupM2,
}

/// Classify a handshake message from its Key Information field and the length
/// of its key data.
///
/// # Why the key data length is needed
///
/// Message 2 and message 4 have *identical* Key Information in the general
/// case: both are pairwise, both carry a MIC, neither sets Key ACK. The Secure
/// bit distinguishes them on an initial association (0 in message 2, 1 in
/// message 4) but not during a rekey, where both are 1. What always separates
/// them is that message 2 carries the station's RSN element and message 4
/// carries nothing at all — so an empty Key Data field means message 4. This
/// is the same discriminator hostapd and wpa_supplicant use, and it is why
/// this function cannot be a pure function of `key_info`.
#[must_use]
pub fn classify(info: u16, key_data_len: usize) -> Option<Message> {
    // An error report or a request is not a handshake message even though it
    // sets the same handful of bits.
    if (info & (key_info::ERROR | key_info::REQUEST | key_info::SMK_MESSAGE)) != 0 {
        return None;
    }
    let ack = (info & key_info::KEY_ACK) != 0;
    let mic = (info & key_info::KEY_MIC) != 0;
    if (info & key_info::PAIRWISE) != 0 {
        match (ack, mic) {
            (true, false) => Some(Message::PairwiseM1),
            (true, true) => Some(Message::PairwiseM3),
            (false, true) if key_data_len == 0 => Some(Message::PairwiseM4),
            (false, true) => Some(Message::PairwiseM2),
            // No ACK and no MIC is not a message any party sends.
            (false, false) => None,
        }
    } else {
        match (ack, mic) {
            (true, true) => Some(Message::GroupM1),
            (false, true) => Some(Message::GroupM2),
            _ => None,
        }
    }
}

/// The fields of an EAPOL-Key frame to be written.
///
/// Grouped into a struct rather than passed as eleven arguments because a call
/// with eleven positional arguments — three of which are 8-octet arrays that
/// the compiler cannot tell apart — is a transposition waiting to happen.
#[derive(Debug, Clone, Copy)]
pub struct KeyFrameFields<'a> {
    /// [`descriptor_type::RSN`] for WPA2/WPA3.
    pub descriptor_type: u8,
    /// The Key Information bitfield in host order; written big-endian.
    pub key_info: u16,
    /// The temporal key length in octets, or 0 when no key is conveyed.
    pub key_len: u16,
    /// The replay counter to send. A reply echoes the counter it answers.
    pub replay_counter: u64,
    /// `SNonce` in message 2; all-zero in message 4.
    pub nonce: [u8; NONCE_LEN],
    /// Zero except in WPA1 group keying.
    pub iv: [u8; IV_LEN],
    /// Zero in every message a supplicant sends.
    pub rsc: [u8; RSC_LEN],
    /// The key data — the station's RSN element in message 2, empty in
    /// message 4.
    pub key_data: &'a [u8],
}

/// Write a complete EAPOL frame (header and EAPOL-Key body) into `out`,
/// returning the number of octets written.
///
/// **The MIC field is written as zeroes.** That is not an omission: the MIC is
/// computed over the finished frame with the MIC field zeroed, so the frame
/// must exist in exactly this form before the MIC can be computed. Compute it
/// over `&out[..n]` and then call [`set_mic`].
///
/// Pass `mic_len = 0` for an AEAD AKM, which has no MIC field.
#[must_use]
pub fn write(
    out: &mut [u8],
    version: u8,
    fields: &KeyFrameFields,
    mic_len: usize,
) -> Option<usize> {
    let body = body_len(mic_len, fields.key_data.len())?;
    let total = HEADER_LEN.checked_add(body)?;
    let buf = out.get_mut(..total)?;

    let hdr = Header {
        version,
        packet_type: packet_type::KEY,
        body_len: u16::try_from(body).ok()?,
    };
    hdr.write(buf)?;

    let b = buf.get_mut(HEADER_LEN..)?;
    *b.get_mut(0)? = fields.descriptor_type;
    b.get_mut(1..3)?
        .copy_from_slice(&fields.key_info.to_be_bytes());
    b.get_mut(3..5)?
        .copy_from_slice(&fields.key_len.to_be_bytes());
    b.get_mut(5..13)?
        .copy_from_slice(&fields.replay_counter.to_be_bytes());
    b.get_mut(13..45)?.copy_from_slice(&fields.nonce);
    b.get_mut(45..61)?.copy_from_slice(&fields.iv);
    b.get_mut(61..69)?.copy_from_slice(&fields.rsc);

    let mic_end = BODY_BEFORE_MIC.checked_add(mic_len)?;
    b.get_mut(BODY_BEFORE_MIC..mic_end)?.fill(0);
    let len_end = mic_end.checked_add(2)?;
    let key_data_len = u16::try_from(fields.key_data.len()).ok()?;
    b.get_mut(mic_end..len_end)?
        .copy_from_slice(&key_data_len.to_be_bytes());
    let data_end = len_end.checked_add(fields.key_data.len())?;
    b.get_mut(len_end..data_end)?
        .copy_from_slice(fields.key_data);

    Some(total)
}

/// Overwrite the Key MIC field of a complete EAPOL frame.
///
/// `frame` is the whole frame including the EAPOL header, as returned by
/// [`write`]. Returns `None` if the frame is too short to hold a MIC of that
/// length.
#[must_use]
pub fn set_mic(frame: &mut [u8], mic: &[u8]) -> Option<()> {
    let end = MIC_OFFSET.checked_add(mic.len())?;
    frame.get_mut(MIC_OFFSET..end)?.copy_from_slice(mic);
    Some(())
}

/// Zero the Key MIC field of a complete EAPOL frame in place, so that the MIC
/// can be computed (or re-computed for verification) over the result.
///
/// Verification is: copy the received frame, note its MIC, `clear_mic` the
/// copy, compute over the copy, compare. Doing it in place on the received
/// buffer would destroy the value being checked, so this takes `&mut` and the
/// caller is expected to have saved the MIC — [`KeyFrame::mic`] borrows it, so
/// in practice the caller copies the frame first.
#[must_use]
pub fn clear_mic(frame: &mut [u8], mic_len: usize) -> Option<()> {
    let end = MIC_OFFSET.checked_add(mic_len)?;
    frame.get_mut(MIC_OFFSET..end)?.fill(0);
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

    /// A message-2 frame as a supplicant would send it: pairwise, MIC set, no
    /// ACK, carrying a two-octet stand-in for an RSN element.
    fn m2_frame(key_data: &[u8]) -> ([u8; 256], usize) {
        let mut out = [0u8; 256];
        let fields = KeyFrameFields {
            descriptor_type: descriptor_type::RSN,
            key_info: u16::from(key_info::VERSION_HMAC_SHA1_AES)
                | key_info::PAIRWISE
                | key_info::KEY_MIC,
            key_len: 16,
            replay_counter: 1,
            nonce: [0x5Au8; NONCE_LEN],
            iv: [0u8; IV_LEN],
            rsc: [0u8; RSC_LEN],
            key_data,
        };
        let n = write(&mut out, version::V2, &fields, MIC_LEN_DEFAULT).expect("fits");
        (out, n)
    }

    #[test]
    fn the_header_is_big_endian() {
        // 0x0100 = 256 as a big-endian u16. Read little-endian it would be 1,
        // which is a plausible-looking length and so would fail silently.
        let frame = [version::V2, packet_type::KEY, 0x01, 0x00];
        let hdr = Header::parse(&frame).expect("parses");
        assert_eq!(hdr.body_len, 256);
        assert_eq!(hdr.packet_type, packet_type::KEY);
    }

    #[test]
    fn key_information_is_big_endian_too() {
        // Key Information 0x008A on the wire: version 2, pairwise, Key ACK —
        // a message 1. Byte-swapped it is 0x8A00, which sets Request and
        // Secure and no version at all.
        let mut raw = [0u8; 8];
        raw[0] = descriptor_type::RSN;
        raw[1] = 0x00;
        raw[2] = 0x8A;
        // Read the field the way the parser does, then interpret *that* value:
        // asserting the flags against the literal would only be checking the
        // constants against themselves at compile time, and would still pass
        // if `be_u16` were byte-swapped.
        let parsed = be_u16(&raw, 1).expect("two octets are present");
        assert_eq!(parsed, 0x008A);
        assert_eq!(key_info::descriptor_version(parsed), 2);
        assert!(parsed & key_info::PAIRWISE != 0);
        assert!(parsed & key_info::KEY_ACK != 0);

        // And the swap is not harmless, which is the reason the endianness of
        // this one field is worth a test at all. Read the other way round,
        // 0x8A00 is a *supplicant* Request on an already-Secure link at no
        // descriptor version, with neither the pairwise nor the ACK bit set —
        // so a byte-swapped parser would not merely misreport this frame, it
        // would route it to the wrong side of the handshake entirely.
        let swapped = parsed.swap_bytes();
        assert_eq!(key_info::descriptor_version(swapped), 0);
        assert!(swapped & key_info::PAIRWISE == 0);
        assert!(swapped & key_info::KEY_ACK == 0);
        assert!(swapped & key_info::REQUEST != 0);
        assert!(swapped & key_info::SECURE != 0);
    }

    #[test]
    fn a_written_frame_parses_back_to_the_same_fields() {
        let rsn_ie = [48u8, 2, 1, 0];
        let (buf, n) = m2_frame(&rsn_ie);
        let body = body(&buf[..n]).expect("body");
        let kf = KeyFrame::parse(body, MIC_LEN_DEFAULT).expect("parses");
        assert_eq!(kf.descriptor_type, descriptor_type::RSN);
        assert_eq!(kf.descriptor_version(), key_info::VERSION_HMAC_SHA1_AES);
        assert_eq!(kf.key_len, 16);
        assert_eq!(kf.replay_counter, 1);
        assert_eq!(kf.nonce, [0x5Au8; NONCE_LEN]);
        assert_eq!(kf.key_data, &rsn_ie[..]);
        assert_eq!(kf.mic.len(), MIC_LEN_DEFAULT);
        assert!(kf.has_mic());
        assert!(kf.is_pairwise());
        assert!(!kf.wants_reply());
    }

    #[test]
    fn a_freshly_written_frame_has_a_zeroed_mic() {
        // The MIC cannot be computed until the frame exists, so `write` must
        // leave the field zero rather than, say, leaving whatever was in the
        // caller's buffer — which would make the MIC depend on uninitialised
        // memory and fail intermittently.
        let mut out = [0xAAu8; 256];
        let fields = KeyFrameFields {
            descriptor_type: descriptor_type::RSN,
            key_info: key_info::PAIRWISE | key_info::KEY_MIC,
            key_len: 16,
            replay_counter: 7,
            nonce: [0u8; NONCE_LEN],
            iv: [0u8; IV_LEN],
            rsc: [0u8; RSC_LEN],
            key_data: &[],
        };
        let n = write(&mut out, version::V2, &fields, MIC_LEN_DEFAULT).expect("fits");
        assert_eq!(
            &out[MIC_OFFSET..MIC_OFFSET + MIC_LEN_DEFAULT],
            &[0u8; MIC_LEN_DEFAULT]
        );
        assert_eq!(n, HEADER_LEN + 77 + MIC_LEN_DEFAULT + 2);
    }

    #[test]
    fn setting_and_clearing_the_mic_hits_exactly_the_mic_field() {
        let (mut buf, n) = m2_frame(&[48, 2, 1, 0]);
        let before = buf;
        let mic = [0xC3u8; MIC_LEN_DEFAULT];
        set_mic(&mut buf[..n], &mic).expect("fits");
        assert_eq!(&buf[MIC_OFFSET..MIC_OFFSET + MIC_LEN_DEFAULT], &mic[..]);
        // Nothing outside the field moved.
        assert_eq!(&buf[..MIC_OFFSET], &before[..MIC_OFFSET]);
        assert_eq!(
            &buf[MIC_OFFSET + MIC_LEN_DEFAULT..n],
            &before[MIC_OFFSET + MIC_LEN_DEFAULT..n]
        );
        clear_mic(&mut buf[..n], MIC_LEN_DEFAULT).expect("fits");
        assert_eq!(&buf[..n], &before[..n]);
    }

    #[test]
    fn trailing_ethernet_padding_is_not_key_data() {
        // The exact bug this guards: a 60-octet minimum Ethernet frame pads a
        // short EAPOL-Key message, and a parser that uses "the rest of the
        // buffer" as the body reads the padding as key data.
        let (buf, n) = m2_frame(&[]);
        let mut padded = [0u8; 256];
        padded[..n].copy_from_slice(&buf[..n]);
        for (i, b) in padded.iter_mut().enumerate().take(n + 20).skip(n) {
            *b = u8::try_from(i & 0xFF).expect("masked");
        }
        let body = body(&padded).expect("body");
        assert_eq!(body.len(), n - HEADER_LEN);
        let kf = KeyFrame::parse(body, MIC_LEN_DEFAULT).expect("parses");
        assert_eq!(kf.key_data, &[] as &[u8]);
    }

    #[test]
    fn a_key_data_length_that_overruns_is_rejected_not_clamped() {
        // Clamping would let an attacker truncate the RSN element the AP sent
        // in message 3 — the downgrade the handshake exists to detect.
        let (mut buf, n) = m2_frame(&[48, 2, 1, 0]);
        let len_at = HEADER_LEN + BODY_BEFORE_MIC + MIC_LEN_DEFAULT;
        buf[len_at..len_at + 2].copy_from_slice(&0xFFFFu16.to_be_bytes());
        let body = body(&buf[..n]).expect("body");
        assert_eq!(KeyFrame::parse(body, MIC_LEN_DEFAULT), None);
    }

    #[test]
    fn a_body_short_by_one_octet_is_rejected_at_every_length() {
        let (buf, n) = m2_frame(&[48, 2, 1, 0]);
        let body = body(&buf[..n]).expect("body");
        for short in 0..body.len() {
            assert!(
                KeyFrame::parse(&body[..short], MIC_LEN_DEFAULT).is_none(),
                "{short} octets must not parse as a full EAPOL-Key body"
            );
        }
        assert!(KeyFrame::parse(body, MIC_LEN_DEFAULT).is_some());
    }

    #[test]
    fn message_two_and_message_four_differ_only_in_their_key_data() {
        // The whole point of `classify` taking a length: these two have
        // identical Key Information during a rekey.
        let info = key_info::PAIRWISE | key_info::KEY_MIC | key_info::SECURE;
        assert_eq!(classify(info, 22), Some(Message::PairwiseM2));
        assert_eq!(classify(info, 0), Some(Message::PairwiseM4));
    }

    #[test]
    fn the_four_way_handshake_classifies_in_order() {
        let m1 = key_info::PAIRWISE | key_info::KEY_ACK;
        let m2 = key_info::PAIRWISE | key_info::KEY_MIC;
        let m3 = key_info::PAIRWISE
            | key_info::KEY_ACK
            | key_info::KEY_MIC
            | key_info::SECURE
            | key_info::INSTALL
            | key_info::ENCRYPTED_KEY_DATA;
        let m4 = key_info::PAIRWISE | key_info::KEY_MIC | key_info::SECURE;
        assert_eq!(classify(m1, 0), Some(Message::PairwiseM1));
        assert_eq!(classify(m2, 22), Some(Message::PairwiseM2));
        assert_eq!(classify(m3, 56), Some(Message::PairwiseM3));
        assert_eq!(classify(m4, 0), Some(Message::PairwiseM4));
    }

    #[test]
    fn the_group_key_handshake_is_not_the_pairwise_one() {
        let g1 =
            key_info::KEY_ACK | key_info::KEY_MIC | key_info::SECURE | key_info::ENCRYPTED_KEY_DATA;
        let g2 = key_info::KEY_MIC | key_info::SECURE;
        assert_eq!(classify(g1, 40), Some(Message::GroupM1));
        assert_eq!(classify(g2, 0), Some(Message::GroupM2));
        // A group message must never be mistaken for a pairwise one: the
        // difference is one bit, and getting it wrong installs the GTK as the
        // PTK.
        assert_ne!(classify(g1, 40), classify(g1 | key_info::PAIRWISE, 40));
    }

    #[test]
    fn an_error_report_is_not_a_handshake_message() {
        // A TKIP Michael failure report sets Pairwise, MIC, Secure, Request
        // and Error. Without the Error check it would classify as message 2
        // or 4 and be fed into the handshake.
        let report = key_info::PAIRWISE
            | key_info::KEY_MIC
            | key_info::SECURE
            | key_info::ERROR
            | key_info::REQUEST;
        assert_eq!(classify(report, 0), None);
    }

    #[test]
    fn the_mic_length_depends_on_the_akm_and_only_on_the_akm() {
        assert_eq!(mic_len_for_akm(akm::PSK), Some(MIC_LEN_DEFAULT));
        assert_eq!(mic_len_for_akm(akm::SAE), Some(MIC_LEN_DEFAULT));
        assert_eq!(mic_len_for_akm(akm::PSK_SHA256), Some(MIC_LEN_DEFAULT));
        assert_eq!(
            mic_len_for_akm(akm::DOT1X_SUITE_B_192),
            Some(MIC_LEN_SUITE_B_192)
        );
        // OWE and FILS use an AEAD and have no MIC field at all; a caller
        // that assumed 16 would find the Key Data Length 16 octets late.
        assert_eq!(mic_len_for_akm(akm::OWE), None);
    }

    #[test]
    fn a_frame_with_no_mic_field_still_round_trips() {
        // The AEAD case: `mic_len` 0 means the Key Data Length sits
        // immediately after the reserved field.
        let mut out = [0u8; 128];
        let fields = KeyFrameFields {
            descriptor_type: descriptor_type::RSN,
            key_info: key_info::PAIRWISE | key_info::KEY_ACK,
            key_len: 16,
            replay_counter: 3,
            nonce: [1u8; NONCE_LEN],
            iv: [0u8; IV_LEN],
            rsc: [0u8; RSC_LEN],
            key_data: &[9, 9, 9],
        };
        let n = write(&mut out, version::V3, &fields, 0).expect("fits");
        assert_eq!(n, HEADER_LEN + 77 + 2 + 3);
        let kf = KeyFrame::parse(body(&out[..n]).expect("body"), 0).expect("parses");
        assert!(kf.mic.is_empty());
        assert_eq!(kf.key_data, &[9, 9, 9]);
    }

    #[test]
    fn writing_into_a_short_buffer_fails_rather_than_truncating() {
        let fields = KeyFrameFields {
            descriptor_type: descriptor_type::RSN,
            key_info: key_info::PAIRWISE | key_info::KEY_MIC,
            key_len: 16,
            replay_counter: 1,
            nonce: [0u8; NONCE_LEN],
            iv: [0u8; IV_LEN],
            rsc: [0u8; RSC_LEN],
            key_data: &[1, 2, 3, 4],
        };
        let needed = HEADER_LEN + 77 + MIC_LEN_DEFAULT + 2 + 4;
        for short in 0..needed {
            let mut out = [0u8; 128];
            assert!(
                write(&mut out[..short], version::V2, &fields, MIC_LEN_DEFAULT).is_none(),
                "{short} octets must not suffice for a {needed}-octet frame"
            );
        }
        let mut out = [0u8; 128];
        assert_eq!(
            write(&mut out[..needed], version::V2, &fields, MIC_LEN_DEFAULT),
            Some(needed)
        );
    }

    #[test]
    fn a_truncated_header_is_not_a_zero_length_body() {
        for short in 0..HEADER_LEN {
            assert_eq!(Header::parse(&[1u8, 3, 0, 0][..short]), None);
            assert_eq!(body(&[1u8, 3, 0, 0][..short]), None);
        }
        // A header that promises more body than is present is an error, not a
        // short read.
        assert_eq!(body(&[version::V2, packet_type::KEY, 0, 8, 1, 2, 3]), None);
    }

    #[test]
    fn the_mic_offset_matches_the_field_layout() {
        // Pinned against the layout rather than restating the sum, so that a
        // change to any fixed field is caught here.
        let layout = 1 + 2 + 2 + REPLAY_COUNTER_LEN + NONCE_LEN + IV_LEN + RSC_LEN + RESERVED_LEN;
        assert_eq!(MIC_OFFSET, HEADER_LEN + layout);
        assert_eq!(
            body_len(MIC_LEN_DEFAULT, 0),
            Some(layout + MIC_LEN_DEFAULT + 2)
        );
    }
}
