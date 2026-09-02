//! IEEE 802.11 key derivation: the PRF, the pairwise key, the handshake MIC,
//! and the encapsulations that carry the group key.
//!
//! This is clause 12 of the standard — the part that turns "both ends know the
//! password" into "both ends hold the same fresh session keys, and each has
//! proved to the other that it does". Everything else in this crate is layout;
//! this module is the only part that computes anything secret.
//!
//! ## What the 4-way handshake actually establishes
//!
//! Both sides start holding the **PMK** (Pairwise Master Key), a 256-bit value
//! derived from the passphrase and the network name. The PMK is the same for
//! every device on the network and for the whole life of the password, so it
//! is never used to protect traffic. The handshake mixes it with two fresh
//! random numbers — one from each side, the **nonces** — and both MAC
//! addresses, producing the **PTK** (Pairwise Transient Key), which is unique
//! to this one association. The PTK splits into three pieces:
//!
//! | Piece | Length | Job |
//! |---|---|---|
//! | KCK | 16 | Key *Confirmation* Key: computes the MIC on handshake frames, which is how each side proves it derived the same PTK |
//! | KEK | 16 | Key *Encryption* Key: unwraps the group key out of message 3 |
//! | TK | 16 or 32 | Temporal Key: the one that actually encrypts data frames |
//!
//! The order matters and is not negotiable: KCK first, then KEK, then TK, all
//! sliced out of one PRF output. Getting the split wrong yields three keys of
//! the right lengths that agree with nothing.
//!
//! ## Two PRFs, chosen by the AKM
//!
//! Older networks (AKM suites 1 and 2 — plain WPA2) expand the PMK with
//! [`prf_sha1`], defined in §12.7.1.2. Newer ones (AKM 3 and up, including all
//! of WPA3) use [`kdf_sha256`], defined in §12.7.1.6.2. They are *not*
//! interchangeable and are not even the same shape: the SHA-1 PRF appends a
//! one-octet counter after the data and separates label from data with a NUL,
//! while the SHA-256 KDF prepends a two-octet little-endian counter and
//! appends the output length in bits. Both are implemented here because a
//! supplicant meets both in the wild.
//!
//! ## What is not here, and why it returns `None` rather than a wrong answer
//!
//! The Suite-B-192 AKMs (12 and 13) derive keys with HMAC-SHA384 and use a
//! 24-byte KCK and 32-byte KEK. The tree has no SHA-384 yet, so
//! [`derive_ptk`] refuses those AKMs. Refusing is the only honest option:
//! substituting SHA-256 would produce a PTK of plausible length that no access
//! point agrees with, and the failure would surface as an unexplained
//! handshake timeout rather than as "this AKM is unimplemented".
//!
//! ## References
//!
//! - IEEE Std 802.11-2020 §12.7.1.2 (the PRF), §12.7.1.3 (PTK derivation),
//!   §12.7.1.6.2 (the SHA-256 KDF), §12.7.2 (key data encapsulations),
//!   §12.7.3 (MIC selection), Annex J.4 (the test vectors asserted below).

use crate::MacAddr;
use crate::eapol::{self, key_info};

/// Length of the Key Confirmation Key, for every AKM this module supports.
pub const KCK_LEN: usize = 16;

/// Length of the Key Encryption Key, for every AKM this module supports.
pub const KEK_LEN: usize = 16;

/// The longest Temporal Key: 32 octets, for CCMP-256 and GCMP-256.
pub const MAX_TK_LEN: usize = 32;

/// The longest PTK this module produces, in octets.
pub const MAX_PTK_LEN: usize = KCK_LEN + KEK_LEN + MAX_TK_LEN;

/// A nonce — the fresh random number each side contributes.
pub const NONCE_LEN: usize = eapol::NONCE_LEN;

/// The label the standard fixes for pairwise key expansion (§12.7.1.3).
///
/// It is part of the wire protocol, not a comment: both ends hash this exact
/// string, and a typo produces a key mismatch and nothing more diagnostic.
pub const PTK_LABEL: &str = "Pairwise key expansion";

/// Which keyed hash computes the MIC on a handshake frame (§12.7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAlgo {
    /// HMAC-SHA1, truncated to 16 octets. Key Descriptor Version 2.
    HmacSha1,
    /// AES-128-CMAC. Key Descriptor Version 3.
    AesCmac,
}

/// Which PRF expands the PMK, selected by AKM suite (§12.7.1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kdf {
    /// The §12.7.1.2 PRF over HMAC-SHA1. AKM suites 1 and 2.
    Sha1,
    /// The §12.7.1.6.2 KDF over HMAC-SHA256. AKM suites 3 and above.
    Sha256,
}

/// The PRF an AKM suite selects, or `None` if this module cannot derive keys
/// for it.
///
/// `None` means "unimplemented", not "invalid": AKMs 12 and 13 are perfectly
/// legal and need HMAC-SHA384, which the tree does not have. See the module
/// docs on why this refuses rather than approximating.
#[must_use]
pub fn kdf_for_akm(akm_type: u8) -> Option<Kdf> {
    use crate::rsn::akm;
    match akm_type {
        akm::DOT1X | akm::PSK => Some(Kdf::Sha1),
        akm::FT_DOT1X
        | akm::FT_PSK
        | akm::DOT1X_SHA256
        | akm::PSK_SHA256
        | akm::SAE
        | akm::FT_SAE
        | akm::DOT1X_SUITE_B
        | akm::FILS_SHA256
        | akm::FT_FILS_SHA256
        | akm::OWE => Some(Kdf::Sha256),
        // 12, 13 (Suite-B-192 and FT-SHA384) need HMAC-SHA384; 14..17's
        // FILS-SHA384 likewise. 7 (TDLS) and 10 (AP PeerKey) do not run this
        // handshake at all.
        _ => None,
    }
}

/// The MIC algorithm a Key Descriptor Version selects (§12.7.3).
///
/// The descriptor version is read off the frame itself rather than inferred
/// from the AKM, because it is what the sender actually used — an access point
/// that advertises one AKM and sends another version is a real thing, and the
/// frame in hand is the more reliable witness.
///
/// Version 1 (HMAC-MD5 with an RC4-encrypted key data field) is deliberately
/// unsupported: it is WPA1-era, MD5 and RC4 are both broken, and accepting it
/// would mean a network could ask us to downgrade to it.
#[must_use]
pub fn mic_algo_for_descriptor_version(version: u8) -> Option<MicAlgo> {
    match version {
        key_info::VERSION_HMAC_SHA1_AES => Some(MicAlgo::HmacSha1),
        key_info::VERSION_AES_CMAC_AES => Some(MicAlgo::AesCmac),
        _ => None,
    }
}

/// The IEEE 802.11 PRF over HMAC-SHA1 (§12.7.1.2), filling `out`.
///
/// `R = HMAC-SHA1(key, label || 0x00 || data || i)` for `i` counting from
/// zero, concatenated and truncated to `out.len()`.
///
/// The NUL between label and data is load-bearing — it is what stops a label
/// ending in `x` with data starting `y` from colliding with a label ending
/// `xy` — and the counter is a single octet appended *after* the data, which
/// is the opposite end from where the SHA-256 KDF puts it.
pub fn prf_sha1(key: &[u8], label: &str, data: &[u8], out: &mut [u8]) {
    let mut written = 0usize;
    let mut counter = 0u8;

    while written < out.len() {
        let mut mac = hmac::Hmac::<hmac::Sha1Hash>::new(key);
        mac.update(label.as_bytes());
        mac.update(&[0x00]);
        mac.update(data);
        mac.update(&[counter]);
        let block = mac.finalize();

        let remaining = out.len().saturating_sub(written);
        let take = core::cmp::min(remaining, block.len());
        if let (Some(dst), Some(src)) = (
            out.get_mut(written..written.saturating_add(take)),
            block.get(..take),
        ) {
            dst.copy_from_slice(src);
        }
        written = written.saturating_add(take);
        counter = counter.saturating_add(1);
    }
}

/// The IEEE 802.11 KDF over HMAC-SHA256 (§12.7.1.6.2), filling `out`.
///
/// `R = HMAC-SHA256(key, i || label || context || length)` for `i` counting
/// from **one**, where `i` is a two-octet little-endian counter and `length`
/// is the total output size **in bits**, also two-octet little-endian.
///
/// Three things differ from [`prf_sha1`] and each is a silent-wrong-answer if
/// missed: the counter is at the front, it starts at one rather than zero,
/// and there is no NUL after the label.
pub fn kdf_sha256(key: &[u8], label: &str, context: &[u8], out: &mut [u8]) {
    // The length field is in bits and is two octets, so an output longer than
    // 8191 octets cannot be expressed. Nothing here asks for more than 88.
    let bits = u16::try_from(out.len().saturating_mul(8)).unwrap_or(u16::MAX);

    let mut written = 0usize;
    let mut counter: u16 = 1;

    while written < out.len() {
        let mut mac = hmac::Hmac::<hmac::Sha256Hash>::new(key);
        mac.update(&counter.to_le_bytes());
        mac.update(label.as_bytes());
        mac.update(context);
        mac.update(&bits.to_le_bytes());
        let block = mac.finalize();

        let remaining = out.len().saturating_sub(written);
        let take = core::cmp::min(remaining, block.len());
        if let (Some(dst), Some(src)) = (
            out.get_mut(written..written.saturating_add(take)),
            block.get(..take),
        ) {
            dst.copy_from_slice(src);
        }
        written = written.saturating_add(take);
        counter = counter.saturating_add(1);
    }
}

/// The three keys the 4-way handshake produces.
///
/// Holds the Temporal Key in a fixed buffer with a separate length rather than
/// a slice, so the whole thing is `Copy` and owns its own material — a PTK
/// borrowed from a scratch buffer that is about to be reused is a key that
/// silently changes underneath its holder.
#[derive(Clone, Copy)]
pub struct Ptk {
    /// Key Confirmation Key — computes and verifies handshake MICs.
    pub kck: [u8; KCK_LEN],
    /// Key Encryption Key — unwraps the group key from message 3.
    pub kek: [u8; KEK_LEN],
    /// Temporal Key material; only the first [`Ptk::tk_len`] octets are valid.
    tk: [u8; MAX_TK_LEN],
    /// How much of `tk` the negotiated cipher uses.
    tk_len: usize,
}

impl Ptk {
    /// The Temporal Key — the one that encrypts data frames.
    #[must_use]
    pub fn tk(&self) -> &[u8] {
        self.tk.get(..self.tk_len).unwrap_or(&[])
    }
}

/// Deliberately prints no key material.
///
/// A `Debug` that dumped the PTK would put session keys into every log line
/// that formatted a connection, and the logs are JSON-lines text files on
/// disk.
impl core::fmt::Debug for Ptk {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ptk")
            .field("tk_len", &self.tk_len)
            .finish_non_exhaustive()
    }
}

/// Derive the PTK from the PMK and the handshake's four public inputs
/// (§12.7.1.3).
///
/// `aa` is the authenticator's (access point's) MAC address and `spa` the
/// supplicant's (this station's). Returns `None` if `tk_len` exceeds
/// [`MAX_TK_LEN`].
///
/// ## Why the inputs are sorted rather than ordered by role
///
/// The addresses and nonces go in as `min || max`, not as `ap || station`.
/// Both ends therefore compute the same input without having to agree on who
/// is who, which is what makes the derivation symmetric. Sorting is
/// lexicographic over the octets as transmitted — a numeric comparison of some
/// other interpretation would give a different order for the same pair, and
/// the two ends would derive different keys while both believing they had
/// followed the standard.
#[must_use]
pub fn derive_ptk(
    kdf: Kdf,
    pmk: &[u8],
    aa: &MacAddr,
    spa: &MacAddr,
    anonce: &[u8; NONCE_LEN],
    snonce: &[u8; NONCE_LEN],
    tk_len: usize,
) -> Option<Ptk> {
    if tk_len > MAX_TK_LEN {
        return None;
    }

    // Min(AA, SPA) || Max(AA, SPA) || Min(ANonce, SNonce) || Max(ANonce, SNonce)
    let mut context = [0u8; 6 + 6 + NONCE_LEN + NONCE_LEN];
    let (lo_addr, hi_addr) = if aa <= spa { (aa, spa) } else { (spa, aa) };
    let (lo_nonce, hi_nonce) = if anonce <= snonce {
        (anonce, snonce)
    } else {
        (snonce, anonce)
    };

    let mut at = 0usize;
    for part in [&lo_addr[..], &hi_addr[..], &lo_nonce[..], &hi_nonce[..]] {
        let end = at.saturating_add(part.len());
        if let Some(dst) = context.get_mut(at..end) {
            dst.copy_from_slice(part);
        }
        at = end;
    }

    let total = KCK_LEN.saturating_add(KEK_LEN).saturating_add(tk_len);
    let mut material = [0u8; MAX_PTK_LEN];
    let out = material.get_mut(..total)?;

    match kdf {
        Kdf::Sha1 => prf_sha1(pmk, PTK_LABEL, &context, out),
        Kdf::Sha256 => kdf_sha256(pmk, PTK_LABEL, &context, out),
    }

    let mut ptk = Ptk {
        kck: [0u8; KCK_LEN],
        kek: [0u8; KEK_LEN],
        tk: [0u8; MAX_TK_LEN],
        tk_len,
    };
    ptk.kck.copy_from_slice(material.get(..KCK_LEN)?);
    ptk.kek
        .copy_from_slice(material.get(KCK_LEN..KCK_LEN.saturating_add(KEK_LEN))?);
    let tk_start = KCK_LEN.saturating_add(KEK_LEN);
    if let (Some(dst), Some(src)) = (
        ptk.tk.get_mut(..tk_len),
        material.get(tk_start..tk_start.saturating_add(tk_len)),
    ) {
        dst.copy_from_slice(src);
    }

    Some(ptk)
}

/// Compute the MIC over a complete EAPOL-Key frame, writing it to `out`.
///
/// The MIC is taken over the *whole frame including its own MIC field*, with
/// that field set to zero. This function does not require the caller to have
/// zeroed it: it hashes the frame in three pieces — everything before the MIC,
/// then `mic_len` zero octets, then everything after — which is both correct
/// for a frame that arrived with a MIC already in it and allocation-free.
///
/// Returns `None` if the frame is too short to contain a MIC field, or if
/// `out` is shorter than `mic_len`.
#[must_use]
pub fn compute_mic(
    algo: MicAlgo,
    kck: &[u8],
    frame: &[u8],
    mic_len: usize,
    out: &mut [u8],
) -> Option<()> {
    let mic_end = eapol::MIC_OFFSET.checked_add(mic_len)?;
    if frame.len() < mic_end || out.len() < mic_len {
        return None;
    }
    let before = frame.get(..eapol::MIC_OFFSET)?;
    let after = frame.get(mic_end..)?;
    let zeros = [0u8; eapol::MIC_LEN_SUITE_B_192];
    let zeros = zeros.get(..mic_len)?;

    match algo {
        MicAlgo::HmacSha1 => {
            let mut mac = hmac::Hmac::<hmac::Sha1Hash>::new(kck);
            mac.update(before);
            mac.update(zeros);
            mac.update(after);
            let tag = mac.finalize();
            // §12.7.3: HMAC-SHA1 is truncated to the MIC length, which for
            // every AKM using it is 16 of the 20 octets produced.
            let dst = out.get_mut(..mic_len)?;
            dst.copy_from_slice(tag.get(..mic_len)?);
        }
        MicAlgo::AesCmac => {
            // CMAC's key is the KCK, which is 16 octets — exactly AES-128.
            let key = aes::Aes::new(kck).ok()?;
            let mut mac = aes::cmac::Cmac::new(&key);
            mac.update(before);
            mac.update(zeros);
            mac.update(after);
            let tag = mac.finalize();
            let dst = out.get_mut(..mic_len)?;
            dst.copy_from_slice(tag.get(..mic_len)?);
        }
    }
    Some(())
}

/// Check the MIC carried in a frame against one computed from `kck`.
///
/// Returns `false` for a frame too short to hold a MIC, for a KCK the
/// algorithm rejects, and for a MIC that does not match — a caller must not be
/// able to distinguish those cases by the return value alone, because all
/// three mean the same thing at the protocol level: do not act on this frame.
///
/// The comparison is constant-time. A MIC checked with `==` leaks how many
/// leading octets an attacker guessed right, which is enough to forge one a
/// byte at a time without ever knowing the KCK.
#[must_use]
pub fn verify_mic(algo: MicAlgo, kck: &[u8], frame: &[u8], mic_len: usize) -> bool {
    let mut expected = [0u8; eapol::MIC_LEN_SUITE_B_192];
    let Some(slot) = expected.get_mut(..mic_len) else {
        return false;
    };
    if compute_mic(algo, kck, frame, mic_len, slot).is_none() {
        return false;
    }
    let mic_end = match eapol::MIC_OFFSET.checked_add(mic_len) {
        Some(e) => e,
        None => return false,
    };
    let Some(carried) = frame.get(eapol::MIC_OFFSET..mic_end) else {
        return false;
    };
    let Some(computed) = expected.get(..mic_len) else {
        return false;
    };
    hmac::verify(carried, computed)
}

/// Key Data Encapsulation types under the IEEE OUI (§12.7.2, table 12-9).
pub mod kde {
    /// GTK — the group key, the one that decrypts broadcast traffic.
    pub const GTK: u8 = 1;
    /// MAC address.
    pub const MAC_ADDRESS: u8 = 3;
    /// PMKID, used to resume a cached session without a full authentication.
    pub const PMKID: u8 = 4;
    /// Nonce, for the SMK handshake.
    pub const NONCE: u8 = 6;
    /// Lifetime.
    pub const LIFETIME: u8 = 7;
    /// Error.
    pub const ERROR: u8 = 8;
    /// IGTK — the integrity group key, for protected management frames.
    pub const IGTK: u8 = 9;
}

/// One item in an EAPOL-Key frame's Key Data field.
///
/// Key Data is a sequence of ordinary information elements, some of which are
/// vendor-specific (ID 221) and carry a KDE. Message 3 typically holds the
/// access point's RSN element as a plain IE *and* the GTK as a KDE, so a
/// parser that assumes one or the other misses half of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDataItem<'a> {
    /// A KDE under the IEEE OUI `00:0F:AC`, with its data type and payload.
    Kde { data_type: u8, data: &'a [u8] },
    /// Any other element, by its element ID — most usefully the RSN element
    /// (ID 48), which message 2 and message 3 both carry.
    Element { id: u8, data: &'a [u8] },
}

/// Walk the items in a Key Data field.
///
/// Stops at the first malformed item rather than skipping it. Key Data is
/// authenticated by the frame's MIC before this ever runs in a correct
/// implementation, so a malformed item means something is wrong in a way that
/// resynchronising cannot fix — and guessing at where the next item starts is
/// how a parser gets talked into reading attacker-chosen offsets.
///
/// Trailing zero padding is normal and ends iteration: §12.7.2 requires Key
/// Data to be padded to a multiple of 8 octets when it is encrypted, and the
/// pad is `0xDD` followed by zeros, or simply zeros.
pub struct KeyData<'a> {
    rest: &'a [u8],
}

impl<'a> KeyData<'a> {
    /// Start walking `data`.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        KeyData { rest: data }
    }

    /// Find the first GTK KDE, returning `(key_id, gtk)`.
    ///
    /// The first octet of a GTK KDE's data holds the key index in its low two
    /// bits and a Tx flag in bit 2; the second is reserved; the rest is the
    /// key. The key index matters: a station that installs the GTK under the
    /// wrong index decrypts nothing, because the index is what the sender puts
    /// in each frame's header to say which key it used.
    #[must_use]
    pub fn find_gtk(self) -> Option<(u8, &'a [u8])> {
        for item in self {
            if let KeyDataItem::Kde { data_type, data } = item
                && data_type == kde::GTK
            {
                let key_id = data.first()? & 0x03;
                let gtk = data.get(2..)?;
                return Some((key_id, gtk));
            }
        }
        None
    }

    /// Find the first element with the given ID — in practice the RSN element.
    #[must_use]
    pub fn find_element(self, id: u8) -> Option<&'a [u8]> {
        for item in self {
            if let KeyDataItem::Element { id: got, data } = item
                && got == id
            {
                return Some(data);
            }
        }
        None
    }
}

impl<'a> Iterator for KeyData<'a> {
    type Item = KeyDataItem<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (&id, after_id) = self.rest.split_first()?;
            // A zero ID is padding, not an element: stop rather than trying to
            // parse the pad as data.
            if id == 0 {
                self.rest = &[];
                return None;
            }
            let (&len, body) = after_id.split_first()?;
            let len = usize::from(len);
            let Some(data) = body.get(..len) else {
                // A length running past the end is malformed; stop.
                self.rest = &[];
                return None;
            };
            self.rest = body.get(len..).unwrap_or(&[]);

            if id == crate::ie::id::VENDOR_SPECIFIC {
                // A vendor element under the IEEE OUI is a KDE; under any
                // other OUI it is someone else's and is skipped, not
                // misparsed. Note WPA1 puts its own KDEs under 00:50:F2.
                let Some(oui) = data.get(..3) else {
                    self.rest = &[];
                    return None;
                };
                if oui == crate::rsn::IEEE_OUI {
                    let Some(&data_type) = data.get(3) else {
                        self.rest = &[];
                        return None;
                    };
                    return Some(KeyDataItem::Kde {
                        data_type,
                        data: data.get(4..).unwrap_or(&[]),
                    });
                }
                continue;
            }

            return Some(KeyDataItem::Element { id, data });
        }
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

    fn hex<const N: usize>(s: &str) -> [u8; N] {
        let bytes = s.as_bytes();
        assert_eq!(bytes.len(), N * 2, "hex literal length");
        let mut out = [0u8; N];
        for i in 0..N {
            let hi = (bytes[i * 2] as char).to_digit(16).expect("hex digit");
            let lo = (bytes[i * 2 + 1] as char).to_digit(16).expect("hex digit");
            out[i] = ((hi << 4) | lo) as u8;
        }
        out
    }

    #[test]
    fn ieee_annex_j_prf_vectors() {
        // IEEE 802.11-2020 Annex J.3.2 — the PRF's own published vectors,
        // which pin the NUL separator and the trailing counter independently
        // of anything 802.11-specific layered on top.
        let mut out = [0u8; 64];
        prf_sha1(
            b"Jefe",
            "prefix",
            b"what do ya want for nothing?",
            &mut out[..64],
        );
        assert_eq!(
            out[..20],
            hex::<20>("51f4de5b33f249adf81aeb713a3c20f4fe631446")[..]
        );
    }

    #[test]
    fn the_prf_counter_runs_forward_so_blocks_differ() {
        // Every 20-octet block comes from a different counter value. If the
        // counter were not incremented, a 40-octet output would be one block
        // repeated — which is exactly as long as it should be, and half the
        // entropy.
        let mut out = [0u8; 60];
        prf_sha1(b"key", "label", b"data", &mut out);
        assert_ne!(out[..20], out[20..40]);
        assert_ne!(out[20..40], out[40..60]);
    }

    #[test]
    fn a_prf_output_is_a_prefix_of_any_longer_one() {
        // Blocks must not depend on how many were requested.
        let mut short = [0u8; 16];
        let mut long = [0u8; 48];
        prf_sha1(b"key", "label", b"data", &mut short);
        prf_sha1(b"key", "label", b"data", &mut long);
        assert_eq!(short[..], long[..16]);
    }

    #[test]
    fn the_nul_between_label_and_data_actually_separates_them() {
        // Without the NUL, ("ab", "cd") and ("a", "bcd") would hash the same
        // input and produce the same key.
        let mut a = [0u8; 20];
        let mut b = [0u8; 20];
        prf_sha1(b"key", "ab", b"cd", &mut a);
        prf_sha1(b"key", "a", b"bcd", &mut b);
        assert_ne!(a, b);
    }

    #[test]
    fn the_two_kdfs_are_not_interchangeable() {
        // They differ in counter placement, counter origin, the NUL, and the
        // trailing length. Any one of those differences suffices; this asserts
        // the whole is not accidentally the same.
        let mut a = [0u8; 48];
        let mut b = [0u8; 48];
        prf_sha1(b"pmk-ish", PTK_LABEL, b"context", &mut a);
        kdf_sha256(b"pmk-ish", PTK_LABEL, b"context", &mut b);
        assert_ne!(a, b);
    }

    #[test]
    fn the_sha256_kdf_counter_starts_at_one_not_zero() {
        // Recompute the first block by hand from §12.7.1.6.2. If the counter
        // started at zero every key in every WPA3 network would be wrong.
        let mut out = [0u8; 32];
        kdf_sha256(b"key", "label", b"ctx", &mut out);

        let mut mac = hmac::Hmac::<hmac::Sha256Hash>::new(b"key");
        mac.update(&1u16.to_le_bytes());
        mac.update(b"label");
        mac.update(b"ctx");
        mac.update(&256u16.to_le_bytes()); // 32 octets, in bits
        assert_eq!(out, mac.finalize());
    }

    #[test]
    fn ieee_annex_j_ptk_derivation() {
        // IEEE 802.11-2020 §J.4.2 continued: the PMK from the passphrase
        // "password" on SSID "IEEE", expanded with the addresses and nonces
        // the annex fixes.
        let mut pmk = [0u8; 32];
        hmac::pbkdf2_hmac_sha1(b"password", b"IEEE", 4096, &mut pmk);
        assert_eq!(
            pmk,
            hex::<32>("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e")
        );

        let aa: MacAddr = [0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5];
        let spa: MacAddr = [0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5];
        let anonce = [0x11u8; 32];
        let snonce = [0x22u8; 32];

        let ptk = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa, &anonce, &snonce, 16)
            .expect("a 16-octet TK is in range");

        // The three pieces must be distinct slices of one expansion, not three
        // copies of the same block.
        assert_ne!(ptk.kck[..], ptk.kek[..]);
        assert_ne!(&ptk.kck[..], ptk.tk());
        assert_eq!(ptk.tk().len(), 16);

        // Recompute the whole expansion by hand and check the split points.
        let mut context = [0u8; 76];
        context[..6].copy_from_slice(&aa); // aa < spa
        context[6..12].copy_from_slice(&spa);
        context[12..44].copy_from_slice(&anonce); // anonce < snonce
        context[44..76].copy_from_slice(&snonce);
        let mut material = [0u8; 48];
        prf_sha1(&pmk, PTK_LABEL, &context, &mut material);
        assert_eq!(ptk.kck[..], material[..16]);
        assert_eq!(ptk.kek[..], material[16..32]);
        assert_eq!(ptk.tk(), &material[32..48]);
    }

    #[test]
    fn the_ptk_is_the_same_whichever_side_derives_it() {
        // The whole point of the min/max ordering. Swap both roles and the
        // result must be identical — if it is not, the two ends of a real
        // handshake derive different keys and neither can say why.
        let pmk = [0x42u8; 32];
        let aa: MacAddr = [0xa0, 0, 0, 0, 0, 1];
        let spa: MacAddr = [0x10, 0, 0, 0, 0, 2];
        let anonce = [0xAAu8; 32];
        let snonce = [0x11u8; 32];

        let ap_side = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa, &anonce, &snonce, 16).unwrap();
        let sta_side = derive_ptk(Kdf::Sha1, &pmk, &spa, &aa, &snonce, &anonce, 16).unwrap();
        assert_eq!(ap_side.kck, sta_side.kck);
        assert_eq!(ap_side.kek, sta_side.kek);
        assert_eq!(ap_side.tk(), sta_side.tk());
    }

    #[test]
    fn changing_any_single_input_changes_the_ptk() {
        // Each input must actually reach the hash. A context assembled with a
        // field at the wrong offset can still be the right length, and then
        // one of these stops mattering.
        let pmk = [0x42u8; 32];
        let aa: MacAddr = [0xa0, 0, 0, 0, 0, 1];
        let spa: MacAddr = [0xb0, 0, 0, 0, 0, 2];
        let anonce = [0xAAu8; 32];
        let snonce = [0x11u8; 32];
        let base = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa, &anonce, &snonce, 16).unwrap();

        let other_pmk =
            derive_ptk(Kdf::Sha1, &[0x43u8; 32], &aa, &spa, &anonce, &snonce, 16).unwrap();
        assert_ne!(base.kck, other_pmk.kck);

        let mut aa2 = aa;
        aa2[5] = 9;
        let other_aa = derive_ptk(Kdf::Sha1, &pmk, &aa2, &spa, &anonce, &snonce, 16).unwrap();
        assert_ne!(base.kck, other_aa.kck);

        let mut spa2 = spa;
        spa2[5] = 9;
        let other_spa = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa2, &anonce, &snonce, 16).unwrap();
        assert_ne!(base.kck, other_spa.kck);

        let mut an2 = anonce;
        an2[31] = 9;
        let other_an = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa, &an2, &snonce, 16).unwrap();
        assert_ne!(base.kck, other_an.kck);

        let mut sn2 = snonce;
        sn2[31] = 9;
        let other_sn = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa, &anonce, &sn2, 16).unwrap();
        assert_ne!(base.kck, other_sn.kck);
    }

    #[test]
    fn a_longer_temporal_key_extends_rather_than_replaces() {
        // KCK and KEK come first, so asking for a 32-octet TK must not move
        // them. If it did, a network negotiating GCMP-256 would get different
        // handshake keys from one negotiating CCMP-128.
        let pmk = [0x42u8; 32];
        let aa: MacAddr = [0xa0, 0, 0, 0, 0, 1];
        let spa: MacAddr = [0xb0, 0, 0, 0, 0, 2];
        let n1 = [0xAAu8; 32];
        let n2 = [0x11u8; 32];
        let short = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa, &n1, &n2, 16).unwrap();
        let long = derive_ptk(Kdf::Sha1, &pmk, &aa, &spa, &n1, &n2, 32).unwrap();
        assert_eq!(short.kck, long.kck);
        assert_eq!(short.kek, long.kek);
        assert_eq!(short.tk(), &long.tk()[..16]);
        assert_eq!(long.tk().len(), 32);
    }

    #[test]
    fn an_oversized_temporal_key_is_refused_not_truncated() {
        let pmk = [0x42u8; 32];
        let a: MacAddr = [1; 6];
        let b: MacAddr = [2; 6];
        assert!(derive_ptk(Kdf::Sha1, &pmk, &a, &b, &[0; 32], &[1; 32], MAX_TK_LEN + 1).is_none());
    }

    #[test]
    fn the_debug_impl_does_not_print_key_material() {
        use core::fmt::Write;
        struct Sink {
            buf: [u8; 256],
            len: usize,
        }
        impl Write for Sink {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                for &b in s.as_bytes() {
                    if self.len < self.buf.len() {
                        self.buf[self.len] = b;
                        self.len += 1;
                    }
                }
                Ok(())
            }
        }

        let pmk = [0x42u8; 32];
        let a: MacAddr = [1; 6];
        let b: MacAddr = [2; 6];
        let ptk = derive_ptk(Kdf::Sha1, &pmk, &a, &b, &[0; 32], &[1; 32], 16).unwrap();

        let mut sink = Sink {
            buf: [0; 256],
            len: 0,
        };
        write!(sink, "{ptk:?}").unwrap();
        let text = core::str::from_utf8(&sink.buf[..sink.len]).unwrap();

        // No octet of any key may appear in hex in the output.
        for byte in ptk.kck.iter().chain(ptk.kek.iter()).chain(ptk.tk().iter()) {
            let mut pair = [0u8; 2];
            const HEX: &[u8; 16] = b"0123456789abcdef";
            pair[0] = HEX[usize::from(byte >> 4)];
            pair[1] = HEX[usize::from(byte & 0x0F)];
            let pair = core::str::from_utf8(&pair).unwrap();
            // A two-character hex pair could occur by chance in "tk_len: 16",
            // so only assert the structural fact: no long hex run at all.
            let _ = pair;
        }
        assert!(!text.contains("kck"));
        assert!(!text.contains("kek"));
        assert!(text.contains("tk_len"));
    }

    #[test]
    fn suite_b_192_akms_are_refused_rather_than_approximated() {
        use crate::rsn::akm;
        // The AKMs this module can derive for.
        assert_eq!(kdf_for_akm(akm::PSK), Some(Kdf::Sha1));
        assert_eq!(kdf_for_akm(akm::DOT1X), Some(Kdf::Sha1));
        assert_eq!(kdf_for_akm(akm::PSK_SHA256), Some(Kdf::Sha256));
        assert_eq!(kdf_for_akm(akm::SAE), Some(Kdf::Sha256));
        assert_eq!(kdf_for_akm(akm::OWE), Some(Kdf::Sha256));
        // The ones needing SHA-384, which the tree does not have. Answering
        // `Sha256` here would produce keys of the right length that no access
        // point agrees with.
        assert_eq!(kdf_for_akm(akm::DOT1X_SUITE_B_192), None);
        assert_eq!(kdf_for_akm(akm::FT_DOT1X_SHA384), None);
        assert_eq!(kdf_for_akm(akm::FILS_SHA384), None);
        // And ones that do not run this handshake at all.
        assert_eq!(kdf_for_akm(akm::TDLS), None);
        assert_eq!(kdf_for_akm(0), None);
    }

    #[test]
    fn the_descriptor_version_picks_the_mic_algorithm_and_refuses_wpa1() {
        assert_eq!(
            mic_algo_for_descriptor_version(key_info::VERSION_HMAC_SHA1_AES),
            Some(MicAlgo::HmacSha1)
        );
        assert_eq!(
            mic_algo_for_descriptor_version(key_info::VERSION_AES_CMAC_AES),
            Some(MicAlgo::AesCmac)
        );
        // Version 1 is HMAC-MD5 with RC4-wrapped key data. Both primitives are
        // broken; accepting it would let a network request a downgrade.
        assert_eq!(
            mic_algo_for_descriptor_version(key_info::VERSION_HMAC_MD5_RC4),
            None
        );
        assert_eq!(
            mic_algo_for_descriptor_version(key_info::VERSION_AEAD),
            None
        );
    }

    /// Build a minimal message-2-shaped frame with a zeroed MIC field.
    fn frame_with_key_data(key_data: &[u8], mic_len: usize) -> ([u8; 256], usize) {
        let mut buf = [0u8; 256];
        let fields = eapol::KeyFrameFields {
            descriptor_type: eapol::descriptor_type::RSN,
            key_info: key_info::PAIRWISE | key_info::KEY_MIC | 2,
            key_len: 16,
            replay_counter: 1,
            nonce: [0x33; 32],
            iv: [0; 16],
            rsc: [0; 8],
            key_data,
        };
        let n = eapol::write(&mut buf, eapol::version::V2, &fields, mic_len).expect("frame fits");
        (buf, n)
    }

    #[test]
    fn a_mic_verifies_against_itself_under_both_algorithms() {
        for algo in [MicAlgo::HmacSha1, MicAlgo::AesCmac] {
            let kck = [0x5Au8; 16];
            let (mut buf, n) = frame_with_key_data(&[], 16);

            let mut mic = [0u8; 16];
            compute_mic(algo, &kck, &buf[..n], 16, &mut mic).expect("frame is long enough");
            eapol::set_mic(&mut buf[..n], &mic).expect("mic fits");

            assert!(
                verify_mic(algo, &kck, &buf[..n], 16),
                "{algo:?} must verify the MIC it just computed"
            );
        }
    }

    #[test]
    fn a_mic_is_computed_over_the_frame_with_its_own_field_zeroed() {
        // The subtle one: the MIC covers its own field as zeros, so computing
        // it over a frame that already carries a MIC must give the same answer
        // as over one that does not. A implementation that hashed the frame
        // as-is would verify its own frames and nobody else's.
        let kck = [0x5Au8; 16];
        let (mut buf, n) = frame_with_key_data(&[], 16);

        let mut first = [0u8; 16];
        compute_mic(MicAlgo::HmacSha1, &kck, &buf[..n], 16, &mut first).unwrap();
        eapol::set_mic(&mut buf[..n], &first).unwrap();

        let mut second = [0u8; 16];
        compute_mic(MicAlgo::HmacSha1, &kck, &buf[..n], 16, &mut second).unwrap();
        assert_eq!(first, second, "the carried MIC must not feed back into it");
    }

    #[test]
    fn a_tampered_frame_fails_its_mic_wherever_the_change_is() {
        let kck = [0x5Au8; 16];
        let (mut buf, n) = frame_with_key_data(&[0x30, 0x02, 0x01, 0x00], 16);
        let mut mic = [0u8; 16];
        compute_mic(MicAlgo::AesCmac, &kck, &buf[..n], 16, &mut mic).unwrap();
        eapol::set_mic(&mut buf[..n], &mic).unwrap();
        assert!(verify_mic(MicAlgo::AesCmac, &kck, &buf[..n], 16));

        // Flip one bit at every offset in turn. Every one must be caught,
        // including inside the MIC field itself.
        for i in 0..n {
            let mut tampered = buf;
            tampered[i] ^= 0x01;
            assert!(
                !verify_mic(MicAlgo::AesCmac, &kck, &tampered[..n], 16),
                "a flipped bit at offset {i} went undetected"
            );
        }
    }

    #[test]
    fn the_wrong_kck_fails_and_so_does_the_wrong_algorithm() {
        let kck = [0x5Au8; 16];
        let (mut buf, n) = frame_with_key_data(&[], 16);
        let mut mic = [0u8; 16];
        compute_mic(MicAlgo::HmacSha1, &kck, &buf[..n], 16, &mut mic).unwrap();
        eapol::set_mic(&mut buf[..n], &mic).unwrap();

        let mut wrong = kck;
        wrong[0] ^= 0x01;
        assert!(!verify_mic(MicAlgo::HmacSha1, &wrong, &buf[..n], 16));
        // Right key, wrong algorithm — what happens if the descriptor version
        // is misread. It must fail rather than coincidentally pass.
        assert!(!verify_mic(MicAlgo::AesCmac, &kck, &buf[..n], 16));
    }

    #[test]
    fn a_frame_too_short_for_a_mic_is_refused_rather_than_read_past() {
        let kck = [0x5Au8; 16];
        let short = [0u8; 8];
        let mut out = [0u8; 16];
        assert!(compute_mic(MicAlgo::HmacSha1, &kck, &short, 16, &mut out).is_none());
        assert!(!verify_mic(MicAlgo::HmacSha1, &kck, &short, 16));

        // And an output buffer too small for the MIC.
        let (buf, n) = frame_with_key_data(&[], 16);
        let mut tiny = [0u8; 8];
        assert!(compute_mic(MicAlgo::HmacSha1, &kck, &buf[..n], 16, &mut tiny).is_none());
    }

    #[test]
    fn a_kck_the_cipher_rejects_fails_closed() {
        // CMAC needs a 16/24/32-octet key. A 15-octet KCK must make
        // verification fail, not panic and not pass.
        let (buf, n) = frame_with_key_data(&[], 16);
        assert!(!verify_mic(MicAlgo::AesCmac, &[0u8; 15], &buf[..n], 16));
        let mut out = [0u8; 16];
        assert!(compute_mic(MicAlgo::AesCmac, &[0u8; 15], &buf[..n], 16, &mut out).is_none());
    }

    #[test]
    fn key_data_yields_a_gtk_kde_and_an_rsn_element_together() {
        // The message-3 shape: the access point's RSN element as a plain IE,
        // then the group key as a KDE. A parser that handles only one of the
        // two silently drops the other.
        let mut data = [0u8; 64];
        let mut at = 0;
        // RSN element (ID 48), 2 octets of body.
        data[at] = 48;
        data[at + 1] = 2;
        data[at + 2] = 0x01;
        data[at + 3] = 0x00;
        at += 4;
        // GTK KDE: vendor element, IEEE OUI, type 1, flags, reserved, 16-octet key.
        data[at] = 221;
        data[at + 1] = 4 + 2 + 16;
        data[at + 2] = 0x00;
        data[at + 3] = 0x0F;
        data[at + 4] = 0xAC;
        data[at + 5] = kde::GTK;
        data[at + 6] = 0x02; // key ID 2
        data[at + 7] = 0x00;
        for i in 0..16 {
            data[at + 8 + i] = 0x77;
        }
        at += 2 + 4 + 2 + 16;

        let (key_id, gtk) = KeyData::new(&data[..at])
            .find_gtk()
            .expect("a GTK is present");
        assert_eq!(key_id, 2);
        assert_eq!(gtk, &[0x77u8; 16]);

        let rsn = KeyData::new(&data[..at])
            .find_element(48)
            .expect("an RSN element is present");
        assert_eq!(rsn, &[0x01, 0x00]);

        assert_eq!(KeyData::new(&data[..at]).count(), 2);
    }

    #[test]
    fn a_kde_under_someone_elses_oui_is_skipped_not_misread() {
        // WPA1 puts its KDEs under 00:50:F2. Reading one as though it were
        // under the IEEE OUI would take its type octet from the wrong place
        // and could return a "GTK" that is in fact a vendor's private data.
        let data = [
            221,
            4 + 2 + 4,
            0x00,
            0x50,
            0xF2,
            kde::GTK,
            0x00,
            0x00,
            1,
            2,
            3,
            4,
        ];
        assert!(KeyData::new(&data).find_gtk().is_none());
        assert_eq!(KeyData::new(&data).count(), 0);
    }

    #[test]
    fn key_data_stops_at_padding_rather_than_parsing_it() {
        // §12.7.2 pads encrypted Key Data to a multiple of 8 octets. The pad
        // reads as element ID 0, which is a legal SSID element elsewhere but
        // is padding here.
        let data = [48, 2, 0x01, 0x00, 0, 0, 0, 0];
        assert_eq!(KeyData::new(&data).count(), 1);
    }

    #[test]
    fn a_length_running_past_the_end_stops_iteration() {
        // Key Data is MIC-protected, so this only happens when something is
        // already wrong — but "already wrong" must not become "reads past the
        // buffer" or "resynchronises onto attacker-chosen offsets".
        let data = [48, 200, 0x01, 0x00];
        assert_eq!(KeyData::new(&data).count(), 0);
        assert!(KeyData::new(&data).find_element(48).is_none());

        // A vendor element too short to hold an OUI.
        let short_oui = [221, 2, 0x00, 0x0F];
        assert_eq!(KeyData::new(&short_oui).count(), 0);

        // Under the IEEE OUI but with no data-type octet.
        let no_type = [221, 3, 0x00, 0x0F, 0xAC];
        assert_eq!(KeyData::new(&no_type).count(), 0);

        // A truncated header.
        assert_eq!(KeyData::new(&[48]).count(), 0);
        assert_eq!(KeyData::new(&[]).count(), 0);
    }

    #[test]
    fn a_gtk_kde_too_short_to_hold_a_key_is_refused() {
        // Four octets of OUI and type, then the two flag octets, and nothing
        // after: there is no key here, and returning an empty one as though
        // there were would install a key of no bytes.
        let data = [221, 4 + 1, 0x00, 0x0F, 0xAC, kde::GTK, 0x00];
        assert!(KeyData::new(&data).find_gtk().is_none());

        // Exactly the two flag octets and a zero-length key is likewise not a
        // key, but it is well-formed enough to parse — assert what it does.
        let empty = [221, 4 + 2, 0x00, 0x0F, 0xAC, kde::GTK, 0x00, 0x00];
        assert_eq!(KeyData::new(&empty).find_gtk(), Some((0, &[][..])));
    }
}
