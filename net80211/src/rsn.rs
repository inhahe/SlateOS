//! The RSN element — how a network advertises which ciphers and which
//! authentication methods it will accept (IEEE 802.11-2020 §9.4.2.24).
//!
//! This is the element that decides whether a network is open, WPA2, WPA3, or
//! enterprise, and it is the one a scanner reads to put a padlock next to a
//! name. Its layout is a version, then a group cipher, then two
//! counted lists — pairwise ciphers and AKM (authentication and key
//! management) suites — then three optional tails:
//!
//! ```text
//! Version (2, LE)
//! Group Data Cipher Suite (4)              ]
//! Pairwise Cipher Count (2, LE) + list     ]  each optional, but only
//! AKM Count (2, LE) + list                 ]  from the end: a field is
//! RSN Capabilities (2, LE)                 ]  present only if every
//! PMKID Count (2, LE) + list (16 each)     ]  field before it is
//! Group Management Cipher Suite (4)        ]
//! ```
//!
//! **Truncation is legal here, and it is not an error.** An element that stops
//! after the version is a well-formed element that means "all defaults"
//! (CCMP-128 for everything). This is the one place in 802.11 where a short
//! buffer must not be rejected — so [`Rsn::parse`] distinguishes *absent*
//! (`None`, take the default) from *malformed* (a count that overruns the
//! element, which is rejected outright).
//!
//! # Suite selectors
//!
//! Every cipher and AKM is a four-octet selector: a three-octet OUI plus a
//! one-octet type. The standard suites use OUI `00:0F:AC`; a vendor may
//! define its own under its own OUI, and a station that does not recognise the
//! OUI must ignore the suite rather than read the type, since type numbers
//! only mean anything within an OUI.

/// The IEEE 802.11 OUI (`00:0F:AC`) under which the standard cipher and AKM
/// suites are defined.
pub const IEEE_OUI: [u8; 3] = [0x00, 0x0F, 0xAC];

/// The only defined RSN element version.
pub const VERSION: u16 = 1;

/// Length of one cipher/AKM suite selector.
pub const SUITE_LEN: usize = 4;
/// Length of one PMKID.
pub const PMKID_LEN: usize = 16;

/// Cipher suite types, within [`IEEE_OUI`] (§9.4.2.24.2, table 9-149).
pub mod cipher {
    /// Use the group cipher (pairwise list only).
    pub const USE_GROUP: u8 = 0;
    /// WEP-40. Broken; present so a scanner can say so.
    pub const WEP40: u8 = 1;
    /// TKIP (WPA1). Deprecated.
    pub const TKIP: u8 = 2;
    /// CCMP-128 (AES-CCM) — the WPA2 default and still the common case.
    pub const CCMP_128: u8 = 4;
    /// WEP-104. Broken.
    pub const WEP104: u8 = 5;
    /// BIP-CMAC-128 — management frame protection.
    pub const BIP_CMAC_128: u8 = 6;
    /// Group addressed traffic not allowed.
    pub const GROUP_NOT_ALLOWED: u8 = 7;
    /// GCMP-128.
    pub const GCMP_128: u8 = 8;
    /// GCMP-256 — the WPA3-Enterprise 192-bit suite's data cipher.
    pub const GCMP_256: u8 = 9;
    /// CCMP-256.
    pub const CCMP_256: u8 = 10;
    /// BIP-GMAC-128.
    pub const BIP_GMAC_128: u8 = 11;
    /// BIP-GMAC-256.
    pub const BIP_GMAC_256: u8 = 12;
    /// BIP-CMAC-256.
    pub const BIP_CMAC_256: u8 = 13;
}

/// AKM (authentication and key management) suite types, within [`IEEE_OUI`]
/// (§9.4.2.24.3, table 9-151).
pub mod akm {
    /// 802.1X / EAP — "WPA2-Enterprise".
    pub const DOT1X: u8 = 1;
    /// Pre-shared key — "WPA2-Personal".
    pub const PSK: u8 = 2;
    /// Fast BSS Transition with 802.1X.
    pub const FT_DOT1X: u8 = 3;
    /// Fast BSS Transition with PSK.
    pub const FT_PSK: u8 = 4;
    /// 802.1X with SHA-256 key derivation.
    pub const DOT1X_SHA256: u8 = 5;
    /// PSK with SHA-256 key derivation.
    pub const PSK_SHA256: u8 = 6;
    /// TDLS (direct link).
    pub const TDLS: u8 = 7;
    /// SAE — "WPA3-Personal".
    pub const SAE: u8 = 8;
    /// Fast BSS Transition with SAE.
    pub const FT_SAE: u8 = 9;
    /// AP PeerKey.
    pub const AP_PEERKEY: u8 = 10;
    /// 802.1X Suite-B.
    pub const DOT1X_SUITE_B: u8 = 11;
    /// 802.1X Suite-B 192-bit — "WPA3-Enterprise 192-bit".
    pub const DOT1X_SUITE_B_192: u8 = 12;
    /// Fast BSS Transition with 802.1X and SHA-384.
    pub const FT_DOT1X_SHA384: u8 = 13;
    /// FILS with SHA-256.
    pub const FILS_SHA256: u8 = 14;
    /// FILS with SHA-384.
    pub const FILS_SHA384: u8 = 15;
    /// Fast BSS Transition FILS with SHA-256.
    pub const FT_FILS_SHA256: u8 = 16;
    /// Fast BSS Transition FILS with SHA-384.
    pub const FT_FILS_SHA384: u8 = 17;
    /// Opportunistic Wireless Encryption — encryption without a password.
    pub const OWE: u8 = 18;
}

/// RSN Capabilities bits (§9.4.2.24.4).
pub mod caps {
    /// Preauthentication supported.
    pub const PREAUTH: u16 = 0x0001;
    /// No Pairwise: the station uses only the group key.
    pub const NO_PAIRWISE: u16 = 0x0002;
    /// Management Frame Protection **Required**.
    pub const MFPR: u16 = 0x0040;
    /// Management Frame Protection **Capable**.
    pub const MFPC: u16 = 0x0080;
    /// PeerKey enabled.
    pub const PEERKEY: u16 = 0x0200;
    /// SPP A-MSDU capable.
    pub const SPP_AMSDU_CAPABLE: u16 = 0x0400;
    /// SPP A-MSDU required.
    pub const SPP_AMSDU_REQUIRED: u16 = 0x0800;
    /// Protected Block Ack Agreement Capable.
    pub const PBAC: u16 = 0x1000;
    /// Extended Key ID for individually addressed frames.
    pub const EXT_KEY_ID: u16 = 0x2000;
    /// Operating Channel Validation Capable.
    pub const OCVC: u16 = 0x4000;
}

/// A four-octet cipher or AKM suite selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Suite(pub [u8; SUITE_LEN]);

impl Suite {
    /// A standard suite: [`IEEE_OUI`] plus `suite_type`.
    #[must_use]
    pub fn standard(suite_type: u8) -> Self {
        Suite([IEEE_OUI[0], IEEE_OUI[1], IEEE_OUI[2], suite_type])
    }

    /// The three-octet OUI.
    #[must_use]
    pub fn oui(self) -> [u8; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    /// The one-octet type. **Only meaningful within an OUI** — check
    /// [`Suite::is_standard`] before comparing against the [`cipher`] or
    /// [`akm`] constants, or a vendor suite numbered 8 will be read as SAE.
    #[must_use]
    pub fn suite_type(self) -> u8 {
        self.0[3]
    }

    /// True if this selector uses the IEEE OUI.
    #[must_use]
    pub fn is_standard(self) -> bool {
        self.oui() == IEEE_OUI
    }

    /// The standard type, or `None` for a vendor suite.
    #[must_use]
    pub fn standard_type(self) -> Option<u8> {
        if self.is_standard() {
            Some(self.suite_type())
        } else {
            None
        }
    }
}

/// The temporal-key length, in octets, for a standard data cipher — the size
/// of the TK slice that has to be carved out of the PTK.
///
/// `None` for a vendor suite or a cipher with no data key of its own.
#[must_use]
pub fn tk_len(suite: Suite) -> Option<usize> {
    match suite.standard_type()? {
        cipher::TKIP => Some(32),
        cipher::CCMP_128 | cipher::GCMP_128 | cipher::BIP_CMAC_128 | cipher::BIP_GMAC_128 => {
            Some(16)
        }
        cipher::GCMP_256 | cipher::CCMP_256 | cipher::BIP_GMAC_256 | cipher::BIP_CMAC_256 => {
            Some(32)
        }
        cipher::WEP40 => Some(5),
        cipher::WEP104 => Some(13),
        _ => None,
    }
}

/// An iterator over a packed list of four-octet suite selectors.
#[derive(Debug, Clone)]
pub struct Suites<'a> {
    rest: &'a [u8],
}

impl Iterator for Suites<'_> {
    type Item = Suite;

    fn next(&mut self) -> Option<Suite> {
        let head = self.rest.get(..SUITE_LEN)?;
        let mut s = [0u8; SUITE_LEN];
        s.copy_from_slice(head);
        self.rest = self.rest.get(SUITE_LEN..).unwrap_or(&[]);
        Some(Suite(s))
    }
}

/// A parsed RSN element body (the data of element ID 48, without the ID and
/// length octets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rsn<'a> {
    /// The version. Anything but [`VERSION`] must be treated as unsupported
    /// rather than guessed at; [`Rsn::parse`] accepts it so a scanner can say
    /// *why* it will not connect.
    pub version: u16,
    /// The group data cipher. `None` means the element stopped early and the
    /// default ([`cipher::CCMP_128`]) applies.
    pub group_cipher: Option<Suite>,
    /// The pairwise cipher list, packed. `None` means "take the default".
    pairwise: Option<&'a [u8]>,
    /// The AKM suite list, packed. `None` means "take the default".
    akms: Option<&'a [u8]>,
    /// RSN Capabilities, if present.
    pub capabilities: Option<u16>,
    /// The PMKID list, packed 16 octets each.
    pmkids: Option<&'a [u8]>,
    /// The group management cipher, for protected management frames.
    pub group_mgmt_cipher: Option<Suite>,
}

impl<'a> Rsn<'a> {
    /// Parse an RSN element body.
    ///
    /// Returns `None` only for a genuinely malformed element: one too short to
    /// hold a version, or one whose declared suite count runs past its end.
    /// An element that simply *stops* at a field boundary is well-formed and
    /// yields `None` for the fields it omits.
    #[must_use]
    pub fn parse(body: &'a [u8]) -> Option<Self> {
        let version = crate::le_u16(body, 0)?;
        let mut off = 2usize;

        let group_cipher = read_suite(body, &mut off);
        if group_cipher.is_none() {
            return Some(Rsn::bare(version));
        }

        let pairwise = match read_list(body, &mut off, SUITE_LEN) {
            ListField::Present(list) => list,
            ListField::Absent => {
                return Some(Rsn {
                    group_cipher,
                    ..Rsn::bare(version)
                });
            }
            ListField::Malformed => return None,
        };
        let akms = match read_list(body, &mut off, SUITE_LEN) {
            ListField::Present(list) => list,
            ListField::Absent => {
                return Some(Rsn {
                    group_cipher,
                    pairwise: Some(pairwise),
                    ..Rsn::bare(version)
                });
            }
            ListField::Malformed => return None,
        };

        let capabilities = crate::le_u16(body, off);
        if capabilities.is_some() {
            off = off.checked_add(2)?;
        }
        let pmkids = if capabilities.is_some() {
            match read_list(body, &mut off, PMKID_LEN) {
                ListField::Present(list) => Some(list),
                ListField::Absent => None,
                ListField::Malformed => return None,
            }
        } else {
            None
        };
        let group_mgmt_cipher = if pmkids.is_some() {
            read_suite(body, &mut off)
        } else {
            None
        };

        Some(Rsn {
            version,
            group_cipher,
            pairwise: Some(pairwise),
            akms: Some(akms),
            capabilities,
            pmkids,
            group_mgmt_cipher,
        })
    }

    fn bare(version: u16) -> Self {
        Rsn {
            version,
            group_cipher: None,
            pairwise: None,
            akms: None,
            capabilities: None,
            pmkids: None,
            group_mgmt_cipher: None,
        }
    }

    /// The advertised pairwise ciphers. An element that omits the list
    /// advertises the default, [`cipher::CCMP_128`], and this iterator is
    /// empty — the *caller* applies the default, because "omitted" and
    /// "explicitly CCMP" are the same policy but not the same bytes, and the
    /// distinction matters when re-encoding an element for a MIC.
    #[must_use]
    pub fn pairwise_ciphers(&self) -> Suites<'a> {
        Suites {
            rest: self.pairwise.unwrap_or(&[]),
        }
    }

    /// The advertised AKM suites. See [`Rsn::pairwise_ciphers`] on defaults.
    #[must_use]
    pub fn akm_suites(&self) -> Suites<'a> {
        Suites {
            rest: self.akms.unwrap_or(&[]),
        }
    }

    /// The PMKIDs offered for fast reconnection, 16 octets each.
    pub fn pmkids(&self) -> impl Iterator<Item = &'a [u8]> {
        self.pmkids.unwrap_or(&[]).chunks_exact(PMKID_LEN)
    }

    /// True if the element advertises any AKM this stack can actually
    /// authenticate with.
    #[must_use]
    pub fn has_supported_akm(&self) -> bool {
        self.akm_suites().any(|s| {
            matches!(
                s.standard_type(),
                Some(akm::PSK | akm::PSK_SHA256 | akm::SAE | akm::FT_PSK | akm::FT_SAE)
            )
        })
    }

    /// True if the network requires protected management frames — mandatory
    /// for WPA3, and the reason a WPA3 network refuses a station that
    /// negotiates without it.
    #[must_use]
    pub fn mfp_required(&self) -> bool {
        self.capabilities.is_some_and(|c| (c & caps::MFPR) != 0)
    }

    /// True if the network *can* protect management frames.
    #[must_use]
    pub fn mfp_capable(&self) -> bool {
        self.capabilities.is_some_and(|c| (c & caps::MFPC) != 0)
    }

    /// True if this is a WPA3-Personal (SAE) network.
    #[must_use]
    pub fn is_sae(&self) -> bool {
        self.akm_suites()
            .any(|s| matches!(s.standard_type(), Some(akm::SAE | akm::FT_SAE)))
    }

    /// True if this is a WPA2-Personal (PSK) network. A transition-mode AP
    /// advertises both PSK and SAE, so this and [`Rsn::is_sae`] can both be
    /// true — which is the point of transition mode, and why neither is
    /// written as an `else` of the other.
    #[must_use]
    pub fn is_psk(&self) -> bool {
        self.akm_suites().any(|s| {
            matches!(
                s.standard_type(),
                Some(akm::PSK | akm::PSK_SHA256 | akm::FT_PSK)
            )
        })
    }
}

/// Read a suite selector at `*off` if there is room, advancing `*off`.
fn read_suite(body: &[u8], off: &mut usize) -> Option<Suite> {
    let end = off.checked_add(SUITE_LEN)?;
    let s = body.get(*off..end)?;
    let mut sel = [0u8; SUITE_LEN];
    sel.copy_from_slice(s);
    *off = end;
    Some(Suite(sel))
}

/// The outcome of reading one optional counted list out of an RSN element.
///
/// Three outcomes, and the difference between the first two is the whole point
/// of the type: an RSN element is a chain of fields each of which may simply
/// stop, so "the element ended here" is normal and means every later field
/// takes its default, whereas "the count says more than is here" is a
/// malformed element and must reject the whole thing.
enum ListField<'a> {
    /// The element ended before this field began. It, and everything after
    /// it, is absent and takes the standard's default.
    Absent,
    /// A well-formed list, possibly of zero items.
    Present(&'a [u8]),
    /// The count overruns the element.
    Malformed,
}

/// Read a `u16` count followed by `count * item_len` octets.
fn read_list<'a>(body: &'a [u8], off: &mut usize, item_len: usize) -> ListField<'a> {
    let Some(count) = crate::le_u16(body, *off) else {
        return ListField::Absent;
    };
    let Some(after_count) = off.checked_add(2) else {
        return ListField::Malformed;
    };
    let Some(bytes) = usize::from(count).checked_mul(item_len) else {
        return ListField::Malformed;
    };
    let Some(end) = after_count.checked_add(bytes) else {
        return ListField::Malformed;
    };
    // A count that runs past the end of the element is malformed. Clamping it
    // to what is present would let a hostile beacon shrink an AKM list to
    // nothing and so make an authenticated network look open.
    let Some(list) = body.get(after_count..end) else {
        return ListField::Malformed;
    };
    *off = end;
    ListField::Present(list)
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// Write an RSN element *body* — version, group cipher, pairwise list, AKM
/// list, capabilities — into `out` at `*off`.
///
/// The optional PMKID list and group management cipher are not emitted: a
/// station's association request needs neither, and an element that omits a
/// field is well-formed (see the module docs).
///
/// Returns the number of octets written, or `None` if it does not fit or a
/// list is longer than `u16::MAX`.
#[must_use]
pub fn write_body(
    out: &mut [u8],
    off: &mut usize,
    group: Suite,
    pairwise: &[Suite],
    akms: &[Suite],
    capabilities: u16,
) -> Option<usize> {
    let start = *off;
    put(out, off, &VERSION.to_le_bytes())?;
    put(out, off, &group.0)?;
    put(out, off, &u16::try_from(pairwise.len()).ok()?.to_le_bytes())?;
    for s in pairwise {
        put(out, off, &s.0)?;
    }
    put(out, off, &u16::try_from(akms.len()).ok()?.to_le_bytes())?;
    for s in akms {
        put(out, off, &s.0)?;
    }
    put(out, off, &capabilities.to_le_bytes())?;
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

    fn wpa2_psk() -> ([u8; 20], usize) {
        let mut buf = [0u8; 20];
        let mut off = 0usize;
        let n = write_body(
            &mut buf,
            &mut off,
            Suite::standard(cipher::CCMP_128),
            &[Suite::standard(cipher::CCMP_128)],
            &[Suite::standard(akm::PSK)],
            0,
        )
        .expect("fits");
        (buf, n)
    }

    #[test]
    fn a_wpa2_psk_element_round_trips() {
        let (buf, n) = wpa2_psk();
        assert_eq!(n, 20);
        let r = Rsn::parse(&buf[..n]).expect("parses");
        assert_eq!(r.version, VERSION);
        assert_eq!(r.group_cipher, Some(Suite::standard(cipher::CCMP_128)));
        assert_eq!(r.pairwise_ciphers().count(), 1);
        assert_eq!(
            r.akm_suites().next().and_then(Suite::standard_type),
            Some(akm::PSK)
        );
        assert_eq!(r.capabilities, Some(0));
        assert!(r.is_psk());
        assert!(!r.is_sae());
        assert!(r.has_supported_akm());
        assert!(!r.mfp_required());
    }

    #[test]
    fn version_and_counts_are_little_endian() {
        let (buf, _) = wpa2_psk();
        assert_eq!(&buf[..2], &[0x01, 0x00], "version 1 is 01 00, not 00 01");
        // Pairwise count follows the 4-octet group cipher.
        assert_eq!(&buf[6..8], &[0x01, 0x00]);
    }

    #[test]
    fn an_element_that_stops_after_the_version_is_valid_and_means_defaults() {
        let r = Rsn::parse(&[0x01, 0x00]).expect("a version-only element is well-formed");
        assert_eq!(r.version, VERSION);
        assert_eq!(r.group_cipher, None);
        assert_eq!(r.pairwise_ciphers().count(), 0);
        assert_eq!(r.akm_suites().count(), 0);
        assert_eq!(r.capabilities, None);
    }

    #[test]
    fn an_element_may_stop_at_any_field_boundary() {
        let (buf, n) = wpa2_psk();
        // 2 = version only, 6 = + group, 12 = + pairwise, 18 = + akm, 20 = all.
        for stop in [2usize, 6, 12, 18, 20] {
            assert!(
                Rsn::parse(&buf[..stop]).is_some(),
                "stopping at {stop} must be accepted"
            );
        }
        assert_eq!(n, 20);
        assert_eq!(Rsn::parse(&buf[..18]).and_then(|r| r.capabilities), None);
        assert_eq!(
            Rsn::parse(&buf[..12]).map(|r| r.akm_suites().count()),
            Some(0)
        );
    }

    #[test]
    fn a_shorter_than_version_element_is_malformed() {
        assert!(Rsn::parse(&[]).is_none());
        assert!(Rsn::parse(&[0x01]).is_none());
    }

    /// The security-relevant case: a count that overruns the element must be
    /// rejected, not clamped. Clamping an AKM count of 9 down to the one suite
    /// actually present — or to none — lets a forged beacon turn a WPA2
    /// network into an open one in the scan list.
    #[test]
    fn a_count_that_overruns_the_element_is_rejected_not_clamped() {
        let mut buf = [0u8; 12];
        buf[0..2].copy_from_slice(&VERSION.to_le_bytes());
        buf[2..6].copy_from_slice(&Suite::standard(cipher::CCMP_128).0);
        buf[6..8].copy_from_slice(&9u16.to_le_bytes()); // claims 9 pairwise suites
        buf[8..12].copy_from_slice(&Suite::standard(cipher::CCMP_128).0);
        assert!(Rsn::parse(&buf).is_none());

        // Same for the AKM list.
        let mut buf = [0u8; 16];
        buf[0..2].copy_from_slice(&VERSION.to_le_bytes());
        buf[2..6].copy_from_slice(&Suite::standard(cipher::CCMP_128).0);
        buf[6..8].copy_from_slice(&1u16.to_le_bytes());
        buf[8..12].copy_from_slice(&Suite::standard(cipher::CCMP_128).0);
        buf[12..14].copy_from_slice(&4u16.to_le_bytes()); // claims 4 AKMs
        assert!(Rsn::parse(&buf).is_none());
    }

    #[test]
    fn a_zero_length_list_is_well_formed() {
        let mut buf = [0u8; 12];
        let mut off = 0usize;
        write_body(
            &mut buf,
            &mut off,
            Suite::standard(cipher::CCMP_128),
            &[],
            &[],
            0,
        )
        .expect("fits");
        let r = Rsn::parse(&buf[..off]).expect("parses");
        assert_eq!(r.pairwise_ciphers().count(), 0);
        assert_eq!(r.akm_suites().count(), 0);
        assert!(
            !r.has_supported_akm(),
            "an empty AKM list authenticates nobody"
        );
    }

    #[test]
    fn wpa3_transition_mode_advertises_both_psk_and_sae() {
        let mut buf = [0u8; 32];
        let mut off = 0usize;
        write_body(
            &mut buf,
            &mut off,
            Suite::standard(cipher::CCMP_128),
            &[Suite::standard(cipher::CCMP_128)],
            &[Suite::standard(akm::PSK), Suite::standard(akm::SAE)],
            caps::MFPC,
        )
        .expect("fits");
        let r = Rsn::parse(&buf[..off]).expect("parses");
        assert!(
            r.is_psk() && r.is_sae(),
            "transition mode is both, not one or the other"
        );
        assert!(r.mfp_capable());
        assert!(
            !r.mfp_required(),
            "requiring MFP would lock out the WPA2 half"
        );
    }

    #[test]
    fn wpa3_only_requires_management_frame_protection() {
        let mut buf = [0u8; 32];
        let mut off = 0usize;
        write_body(
            &mut buf,
            &mut off,
            Suite::standard(cipher::CCMP_128),
            &[Suite::standard(cipher::CCMP_128)],
            &[Suite::standard(akm::SAE)],
            caps::MFPC | caps::MFPR,
        )
        .expect("fits");
        let r = Rsn::parse(&buf[..off]).expect("parses");
        assert!(r.is_sae() && !r.is_psk());
        assert!(r.mfp_required() && r.mfp_capable());
    }

    /// A vendor suite numbered 8 is not SAE. Comparing the type octet without
    /// first checking the OUI is how a vendor extension becomes a spurious
    /// WPA3 badge.
    #[test]
    fn a_vendor_suite_is_not_read_as_a_standard_one() {
        let vendor = Suite([0x00, 0x50, 0xF2, akm::SAE]);
        assert!(!vendor.is_standard());
        assert_eq!(vendor.standard_type(), None);
        assert_eq!(vendor.suite_type(), akm::SAE);

        let mut buf = [0u8; 32];
        let mut off = 0usize;
        write_body(
            &mut buf,
            &mut off,
            Suite::standard(cipher::CCMP_128),
            &[Suite::standard(cipher::CCMP_128)],
            &[vendor],
            0,
        )
        .expect("fits");
        let r = Rsn::parse(&buf[..off]).expect("parses");
        assert!(!r.is_sae());
        assert!(!r.has_supported_akm());
    }

    #[test]
    fn temporal_key_lengths_match_the_ciphers() {
        assert_eq!(tk_len(Suite::standard(cipher::CCMP_128)), Some(16));
        assert_eq!(tk_len(Suite::standard(cipher::GCMP_256)), Some(32));
        assert_eq!(tk_len(Suite::standard(cipher::CCMP_256)), Some(32));
        assert_eq!(tk_len(Suite::standard(cipher::TKIP)), Some(32));
        assert_eq!(tk_len(Suite::standard(cipher::WEP40)), Some(5));
        assert_eq!(tk_len(Suite::standard(cipher::WEP104)), Some(13));
        assert_eq!(tk_len(Suite::standard(cipher::USE_GROUP)), None);
        assert_eq!(tk_len(Suite([0x00, 0x50, 0xF2, cipher::CCMP_128])), None);
    }

    #[test]
    fn capabilities_pmkids_and_group_management_cipher_parse_together() {
        let mut buf = [0u8; 64];
        let mut off = 0usize;
        write_body(
            &mut buf,
            &mut off,
            Suite::standard(cipher::CCMP_128),
            &[Suite::standard(cipher::CCMP_128)],
            &[Suite::standard(akm::SAE)],
            caps::MFPC | caps::MFPR,
        )
        .expect("fits");
        // One PMKID, then the group management cipher.
        buf[off..off + 2].copy_from_slice(&1u16.to_le_bytes());
        off += 2;
        buf[off..off + PMKID_LEN].copy_from_slice(&[0xA5u8; PMKID_LEN]);
        off += PMKID_LEN;
        buf[off..off + SUITE_LEN].copy_from_slice(&Suite::standard(cipher::BIP_CMAC_128).0);
        off += SUITE_LEN;

        let r = Rsn::parse(&buf[..off]).expect("parses");
        assert_eq!(r.pmkids().count(), 1);
        assert_eq!(
            r.pmkids().next().and_then(|p| p.first().copied()),
            Some(0xA5)
        );
        assert_eq!(
            r.group_mgmt_cipher,
            Some(Suite::standard(cipher::BIP_CMAC_128))
        );
    }

    #[test]
    fn a_pmkid_count_that_overruns_is_rejected() {
        let mut buf = [0u8; 32];
        let mut off = 0usize;
        write_body(
            &mut buf,
            &mut off,
            Suite::standard(cipher::CCMP_128),
            &[Suite::standard(cipher::CCMP_128)],
            &[Suite::standard(akm::PSK)],
            0,
        )
        .expect("fits");
        buf[off..off + 2].copy_from_slice(&3u16.to_le_bytes());
        off += 2;
        assert!(Rsn::parse(&buf[..off]).is_none());
    }

    #[test]
    fn an_unsupported_version_still_parses_so_the_reason_can_be_reported() {
        let mut buf = [0u8; 20];
        let (src, n) = wpa2_psk();
        buf[..n].copy_from_slice(&src[..n]);
        buf[0..2].copy_from_slice(&2u16.to_le_bytes());
        let r = Rsn::parse(&buf[..n]).expect("parses");
        assert_eq!(r.version, 2);
        assert_ne!(r.version, VERSION, "the caller decides; the parser reports");
    }

    #[test]
    fn building_into_a_short_buffer_fails_rather_than_truncating() {
        for short in 0..20 {
            let mut buf = [0u8; 20];
            let mut off = 0usize;
            assert!(
                write_body(
                    &mut buf[..short],
                    &mut off,
                    Suite::standard(cipher::CCMP_128),
                    &[Suite::standard(cipher::CCMP_128)],
                    &[Suite::standard(akm::PSK)],
                    0,
                )
                .is_none(),
                "{short} octets must not suffice"
            );
        }
    }
}
