//! Information elements — the tag-length-value chain that makes up the
//! variable part of every management frame (IEEE 802.11-2020 §9.4.2).
//!
//! ```text
//! +--------+--------+------------------+
//! | ID (1) | Len(1) | Data (Len bytes) |
//! +--------+--------+------------------+
//! ```
//!
//! with one wrinkle: ID 255 is *Element ID Extension*, and its first data
//! octet is a second identifier. The length still counts that octet, so an
//! extension element's payload is one shorter than its declared length. Both
//! forms are produced by [`Elements`], which reports the extension identifier
//! separately and hands back only the payload.
//!
//! # Truncation is the normal case
//!
//! A beacon that has been cut short mid-element is not exotic — it is what
//! arrives when a frame is clipped by a receive buffer, and it is what an
//! attacker sends on purpose. [`Elements`] therefore *stops* at the first
//! element whose declared length runs past the end of the buffer, rather than
//! yielding a short element or panicking. A caller that needs to know whether
//! the chain was well-formed can ask [`Elements::is_truncated`] after the
//! iterator is exhausted.

/// Element IDs (§9.4.2, table 9-128). Only the ones this project needs are
/// named; the rest are passed through by [`Elements`] as raw IDs.
pub mod id {
    /// SSID (network name), 0–32 octets.
    pub const SSID: u8 = 0;
    /// Supported Rates, up to 8 rates.
    pub const SUPPORTED_RATES: u8 = 1;
    /// DSSS Parameter Set — carries the channel number.
    pub const DS_PARAMETER_SET: u8 = 3;
    /// Traffic Indication Map (beacon only).
    pub const TIM: u8 = 5;
    /// Country.
    pub const COUNTRY: u8 = 7;
    /// Power Constraint.
    pub const POWER_CONSTRAINT: u8 = 32;
    /// HT Capabilities (802.11n).
    pub const HT_CAPABILITIES: u8 = 45;
    /// RSN — the WPA2/WPA3 cipher and AKM advertisement. See [`crate::rsn`].
    pub const RSN: u8 = 48;
    /// Extended Supported Rates — the rates that did not fit in element 1.
    pub const EXTENDED_SUPPORTED_RATES: u8 = 50;
    /// HT Operation (802.11n).
    pub const HT_OPERATION: u8 = 61;
    /// VHT Capabilities (802.11ac).
    pub const VHT_CAPABILITIES: u8 = 191;
    /// VHT Operation (802.11ac).
    pub const VHT_OPERATION: u8 = 192;
    /// Vendor Specific. WPA1 lives here, under OUI `00:50:F2` type 1, because
    /// it predates the RSN element being standardised.
    pub const VENDOR_SPECIFIC: u8 = 221;
    /// Element ID Extension — the first data octet is a second identifier.
    pub const EXTENSION: u8 = 255;
}

/// The `00:50:F2` (Microsoft) OUI, under which the pre-standard WPA1 element
/// is carried as a vendor-specific element with type 1.
pub const WPA1_OUI: [u8; 3] = [0x00, 0x50, 0xF2];
/// The vendor-specific type that identifies the WPA1 element.
pub const WPA1_OUI_TYPE: u8 = 1;

/// One parsed information element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element<'a> {
    /// The element ID. `255` for an extension element, in which case
    /// [`Element::ext_id`] carries the real identifier.
    pub id: u8,
    /// The extension identifier, for elements with `id == 255`.
    pub ext_id: Option<u8>,
    /// The element payload, *excluding* the extension identifier octet.
    pub data: &'a [u8],
}

impl Element<'_> {
    /// True if this element has the given ID and is not an extension element.
    #[must_use]
    pub fn is(&self, id: u8) -> bool {
        self.id == id && self.ext_id.is_none()
    }

    /// True if this is the WPA1 vendor-specific element (OUI `00:50:F2`,
    /// type 1).
    #[must_use]
    pub fn is_wpa1(&self) -> bool {
        self.id == id::VENDOR_SPECIFIC
            && self.data.len() >= 4
            && self.data.get(..3) == Some(&WPA1_OUI[..])
            && self.data.get(3) == Some(&WPA1_OUI_TYPE)
    }
}

/// An iterator over the information elements in a management-frame body.
#[derive(Debug, Clone)]
pub struct Elements<'a> {
    rest: &'a [u8],
    truncated: bool,
}

impl<'a> Elements<'a> {
    /// Start iterating the element chain at the front of `buf`.
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Elements {
            rest: buf,
            truncated: false,
        }
    }

    /// True if iteration stopped because an element's declared length ran past
    /// the end of the buffer.
    ///
    /// Meaningful only once the iterator is exhausted. Callers that must
    /// reject malformed frames outright — anything security-relevant, such as
    /// reading an RSN element to decide a cipher — should check this, because
    /// "the element I wanted was not present" and "the frame was cut off
    /// before it" are not the same statement.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// The first element with the given ID, or `None`.
    ///
    /// Duplicate elements are legal in some frames and illegal in others; this
    /// returns the first, which is what the standard's "the element" wording
    /// means wherever only one is permitted.
    #[must_use]
    pub fn find_id(buf: &'a [u8], id: u8) -> Option<Element<'a>> {
        Elements::new(buf).find(|e| e.is(id))
    }

    /// The first extension element with the given extension ID, or `None`.
    #[must_use]
    pub fn find_ext_id(buf: &'a [u8], ext_id: u8) -> Option<Element<'a>> {
        Elements::new(buf).find(|e| e.id == id::EXTENSION && e.ext_id == Some(ext_id))
    }
}

impl<'a> Iterator for Elements<'a> {
    type Item = Element<'a>;

    fn next(&mut self) -> Option<Element<'a>> {
        let id = *self.rest.first()?;
        let Some(&len) = self.rest.get(1) else {
            // A lone ID octet with no length: the chain was cut in half.
            self.truncated = true;
            self.rest = &[];
            return None;
        };
        let len = usize::from(len);
        let body_end = 2usize.checked_add(len)?;
        let Some(body) = self.rest.get(2..body_end) else {
            self.truncated = true;
            self.rest = &[];
            return None;
        };
        // `body_end <= rest.len()` because the `get` above succeeded.
        self.rest = self.rest.get(body_end..).unwrap_or(&[]);

        if id == id::EXTENSION {
            // The extension identifier is counted by `len`, so an extension
            // element with a declared length of zero has no identifier and is
            // malformed rather than empty.
            let Some((&ext_id, payload)) = body.split_first() else {
                self.truncated = true;
                self.rest = &[];
                return None;
            };
            return Some(Element {
                id,
                ext_id: Some(ext_id),
                data: payload,
            });
        }
        Some(Element {
            id,
            ext_id: None,
            data: body,
        })
    }
}

// ---------------------------------------------------------------------------
// Typed readers for the elements this project acts on
// ---------------------------------------------------------------------------

/// The SSID carried by a management frame body, as raw octets.
///
/// Returns `None` if there is no SSID element or it is longer than the
/// 32-octet maximum. An SSID is **not** decoded as UTF-8 anywhere in this
/// crate: the standard permits any octet string, and lossy decoding would turn
/// two different networks into one name.
#[must_use]
pub fn ssid(body: &[u8]) -> Option<&[u8]> {
    let e = Elements::find_id(body, id::SSID)?;
    if e.data.len() > crate::MAX_SSID_LEN {
        None
    } else {
        Some(e.data)
    }
}

/// True if the SSID element marks a hidden network.
///
/// Two encodings are in use and both must be recognised: a zero-length SSID
/// element, and an element of the right length filled with NULs. A scanner
/// that handles only the first shows a row of blank names for the second.
#[must_use]
pub fn ssid_is_hidden(ssid: &[u8]) -> bool {
    ssid.is_empty() || ssid.iter().all(|&b| b == 0)
}

/// The channel number from the DSSS Parameter Set element, if present.
#[must_use]
pub fn channel(body: &[u8]) -> Option<u8> {
    let e = Elements::find_id(body, id::DS_PARAMETER_SET)?;
    // The element is defined as exactly one octet; a longer one is malformed,
    // and taking its first octet anyway would be guessing.
    if e.data.len() == 1 {
        e.data.first().copied()
    } else {
        None
    }
}

/// Bit 7 of a rate octet: this rate is a *basic* rate, one a station must
/// support to join the BSS.
pub const RATE_BASIC_BIT: u8 = 0x80;

/// Decode one Supported Rates octet into units of 500 kbit/s and a basic flag.
///
/// The rate is a 7-bit field; bit 7 is the basic-rate marker and is *not* part
/// of the number. Treating the octet as a whole makes every basic rate come
/// out 64 Mbit/s too fast.
#[must_use]
pub fn rate_500kbps(octet: u8) -> (u8, bool) {
    (octet & !RATE_BASIC_BIT, (octet & RATE_BASIC_BIT) != 0)
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// Write one information element into `out` at `*off`, advancing `*off`.
///
/// `None` if `data` is longer than 255 octets — which cannot be expressed — or
/// if the element does not fit in the remaining buffer.
#[must_use]
pub fn write_element(out: &mut [u8], off: &mut usize, id: u8, data: &[u8]) -> Option<usize> {
    let len = u8::try_from(data.len()).ok()?;
    let total = data.len().checked_add(2)?;
    let end = off.checked_add(total)?;
    let dst = out.get_mut(*off..end)?;
    *dst.get_mut(0)? = id;
    *dst.get_mut(1)? = len;
    dst.get_mut(2..)?.copy_from_slice(data);
    *off = end;
    Some(total)
}

/// Write an SSID element. An empty `ssid` produces the zero-length element
/// that means "wildcard" in a probe request and "hidden" in a beacon.
#[must_use]
pub fn write_ssid(out: &mut [u8], off: &mut usize, ssid: &[u8]) -> Option<usize> {
    if ssid.len() > crate::MAX_SSID_LEN {
        return None;
    }
    write_element(out, off, id::SSID, ssid)
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

    /// A beacon tail: SSID "slate", supported rates, channel 6.
    const BODY: &[u8] = &[
        0x00, 0x05, b's', b'l', b'a', b't', b'e', // SSID
        0x01, 0x04, 0x82, 0x84, 0x0B, 0x16, // rates 1(B) 2(B) 5.5 11
        0x03, 0x01, 0x06, // channel 6
    ];

    #[test]
    fn iterates_a_well_formed_chain() {
        let mut it = Elements::new(BODY);

        let e = it.next().expect("ssid");
        assert_eq!(e.id, id::SSID);
        assert_eq!(e.data, b"slate");

        let e = it.next().expect("rates");
        assert_eq!(e.id, id::SUPPORTED_RATES);
        assert_eq!(e.data.len(), 4);

        let e = it.next().expect("channel");
        assert_eq!(e.id, id::DS_PARAMETER_SET);
        assert_eq!(e.data, &[6]);

        assert!(it.next().is_none());
        assert!(!it.is_truncated());
    }

    #[test]
    fn typed_readers() {
        assert_eq!(ssid(BODY), Some(&b"slate"[..]));
        assert_eq!(channel(BODY), Some(6));
        assert_eq!(rate_500kbps(0x82), (2, true)); // 1 Mbit/s, basic
        assert_eq!(rate_500kbps(0x16), (22, false)); // 11 Mbit/s, not basic
    }

    #[test]
    fn a_basic_rate_is_not_sixty_four_megabits_faster() {
        // 0x8C is 6 Mbit/s marked basic (12 * 500 kbit/s). Read whole, it is
        // 140 units = 70 Mbit/s, which is not a rate 802.11a can express.
        let (units, basic) = rate_500kbps(0x8C);
        assert_eq!(units, 12);
        assert!(basic);
    }

    #[test]
    fn hidden_ssids_are_recognised_in_both_encodings() {
        assert!(ssid_is_hidden(b""));
        assert!(ssid_is_hidden(&[0u8; 8]));
        assert!(!ssid_is_hidden(b"slate"));
        // A name that merely *contains* a NUL is not hidden.
        assert!(!ssid_is_hidden(b"sla\0te"));
    }

    #[test]
    fn a_truncated_element_stops_iteration_and_is_reported() {
        // The SSID element claims 5 octets but only 3 are present.
        let cut = &BODY[..5];
        let mut it = Elements::new(cut);
        assert!(it.next().is_none());
        assert!(it.is_truncated());

        // ...and the elements before the cut are still yielded.
        let cut = &BODY[..12];
        let mut it = Elements::new(cut);
        assert_eq!(it.next().map(|e| e.id), Some(id::SSID));
        assert!(it.next().is_none());
        assert!(it.is_truncated());
    }

    #[test]
    fn a_lone_id_octet_is_truncation_not_an_element() {
        let mut it = Elements::new(&[id::SSID]);
        assert!(it.next().is_none());
        assert!(it.is_truncated());
    }

    #[test]
    fn an_exhausted_chain_is_not_truncated() {
        let mut it = Elements::new(&[]);
        assert!(it.next().is_none());
        assert!(
            !it.is_truncated(),
            "no elements at all is well-formed, not cut short"
        );
    }

    #[test]
    fn extension_elements_split_off_their_second_identifier() {
        // ID 255, length 3 (ext id + 2 payload octets).
        let buf = [255u8, 3, 36, 0xAA, 0xBB];
        let e = Elements::new(&buf).next().expect("element");
        assert_eq!(e.id, id::EXTENSION);
        assert_eq!(e.ext_id, Some(36));
        assert_eq!(
            e.data,
            &[0xAA, 0xBB],
            "the extension id must not stay in the payload"
        );
        assert_eq!(
            Elements::find_ext_id(&buf, 36).map(|e| e.data),
            Some(&[0xAA, 0xBB][..])
        );
        // `find` by plain ID must not match an extension element, or every
        // lookup for element 255 would return the first extension of any kind.
        assert!(Elements::find_id(&buf, 255).is_none());
    }

    #[test]
    fn a_zero_length_extension_element_is_malformed() {
        // Length 0 leaves no room for the extension identifier the length is
        // defined to include.
        let mut it = Elements::new(&[255u8, 0]);
        assert!(it.next().is_none());
        assert!(it.is_truncated());
    }

    #[test]
    fn wpa1_vendor_element_is_recognised_and_others_are_not() {
        let wpa1 = [221u8, 6, 0x00, 0x50, 0xF2, 0x01, 0x01, 0x00];
        assert!(Elements::new(&wpa1).next().expect("element").is_wpa1());

        // Same OUI, different type (2 is WMM, not WPA).
        let wmm = [221u8, 6, 0x00, 0x50, 0xF2, 0x02, 0x00, 0x01];
        assert!(!Elements::new(&wmm).next().expect("element").is_wpa1());

        // Right type, different OUI.
        let other = [221u8, 6, 0x00, 0x0F, 0xAC, 0x01, 0x01, 0x00];
        assert!(!Elements::new(&other).next().expect("element").is_wpa1());

        // Too short to hold an OUI and a type.
        let stub = [221u8, 3, 0x00, 0x50, 0xF2];
        assert!(!Elements::new(&stub).next().expect("element").is_wpa1());
    }

    #[test]
    fn an_over_long_ssid_element_is_refused() {
        let mut buf = [0u8; 2 + 33];
        buf[0] = id::SSID;
        buf[1] = 33;
        assert!(
            ssid(&buf).is_none(),
            "33 octets exceeds the 32-octet maximum"
        );
        // The iterator still yields it — length policy is the reader's job,
        // not the chain walker's.
        assert_eq!(Elements::new(&buf).next().map(|e| e.data.len()), Some(33));
    }

    #[test]
    fn a_multi_octet_channel_element_is_refused_rather_than_guessed() {
        let buf = [id::DS_PARAMETER_SET, 2, 6, 0];
        assert!(channel(&buf).is_none());
    }

    #[test]
    fn building_round_trips_through_parsing() {
        let mut buf = [0u8; 32];
        let mut off = 0usize;
        assert_eq!(write_ssid(&mut buf, &mut off, b"slate"), Some(7));
        assert_eq!(
            write_element(&mut buf, &mut off, id::DS_PARAMETER_SET, &[6]),
            Some(3)
        );
        assert_eq!(off, 10);
        assert_eq!(ssid(&buf[..off]), Some(&b"slate"[..]));
        assert_eq!(channel(&buf[..off]), Some(6));
    }

    #[test]
    fn building_refuses_what_it_cannot_express_or_fit() {
        let mut buf = [0u8; 8];
        let mut off = 0usize;
        assert!(write_element(&mut buf, &mut off, id::SSID, &[0u8; 16]).is_none());
        assert_eq!(off, 0, "a failed write must not advance the offset");

        let mut big = [0u8; 300];
        let mut off = 0usize;
        assert!(
            write_element(&mut big, &mut off, id::SSID, &[0u8; 256]).is_none(),
            "256 octets cannot be expressed in a one-octet length"
        );

        let mut buf = [0u8; 64];
        let mut off = 0usize;
        assert!(write_ssid(&mut buf, &mut off, &[b'x'; 33]).is_none());
    }
}
