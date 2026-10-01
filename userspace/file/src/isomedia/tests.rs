//! `file -b` and `file -b --mime-type` of file 5.45, measured in WSL
//! (Ubuntu 24.04's file 5.45, magic sha256 d5b7d238...) on 2026-10-01, for
//! buffers built exactly as these tests build them. `scripts/file-isomedia-diff.sh`
//! runs the comparison live over every brand in the table.

#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects
)]

use super::identify;

/// A 24-byte `ftyp` box naming `brand` (as major and first compatible
/// brand, then `mif1`), and `pad` zero bytes after it.
fn ftyp(brand: &[u8; 4], pad: usize) -> Vec<u8> {
    let mut b = 24u32.to_be_bytes().to_vec();
    b.extend_from_slice(b"ftyp");
    b.extend_from_slice(brand);
    b.extend_from_slice(&0u32.to_be_bytes());
    b.extend_from_slice(brand);
    b.extend_from_slice(b"mif1");
    b.resize(24 + pad, 0);
    b
}

fn check(buf: &[u8], desc: &str, mime: &str) {
    assert_eq!(
        identify(buf),
        (desc.to_string(), mime),
        "{:?}",
        &buf[..buf.len().min(12)]
    );
}

#[test]
fn every_brand_measured_against_file_5_45() {
    let cases: &[(&[u8; 4], &str, &str)] = &[
        (b"avif", "ISO Media, AVIF Image", "image/avif"),
        (b"avis", "ISO Media, AVIF Image Sequence", "image/avif"),
        (
            b"heic",
            "ISO Media, HEIF Image HEVC Main or Main Still Picture Profile",
            "image/heic",
        ),
        (
            b"heix",
            "ISO Media, HEIF Image HEVC Main 10 Profile",
            "image/heic",
        ),
        // "Sequenz" is file 5.45's own spelling, and so this one's.
        (
            b"hevc",
            "ISO Media, HEIF Image Sequenz HEVC Main or Main Still Picture Profile",
            "image/heic-sequence",
        ),
        (
            b"hevx",
            "ISO Media, HEIF Image Sequence HEVC Main 10 Profile",
            "image/heic-sequence",
        ),
        (b"heim", "ISO Media, HEIF Image L-HEVC", "image/heif"),
        (b"heis", "ISO Media, HEIF Image L-HEVC", "image/heif"),
        (b"avic", "ISO Media, HEIF Image AVC", "image/heif"),
        (b"mif1", "ISO Media, HEIF Image", "image/heif"),
        (
            b"msf1",
            "ISO Media, HEIF Image Sequence",
            "image/heif-sequence",
        ),
        (
            b"isom",
            "ISO Media, MP4 Base Media v1 [ISO 14496-12:2003]",
            "video/mp4",
        ),
        (
            b"iso2",
            "ISO Media, MP4 Base Media v2 [ISO 14496-12:2005]",
            "video/mp4",
        ),
        (b"iso ", "ISO Media, MP4 Base Media", "video/mp4"),
        (b"mp41", "ISO Media, MP4 v1 [ISO 14496-1:ch13]", "video/mp4"),
        (b"mp42", "ISO Media, MP4 v2 [ISO 14496-14]", "video/mp4"),
        (
            b"avc1",
            "ISO Media, MPEG v4 system, 3GPP JVT AVC [ISO 14496-12:2005]",
            "video/mp4",
        ),
        (
            b"dash",
            "ISO Media, MPEG v4 system, Dynamic Adaptive Streaming over HTTP",
            "video/mp4",
        ),
        (
            b"M4A ",
            "ISO Media, Apple iTunes ALAC/AAC-LC (.M4A) Audio",
            "audio/x-m4a",
        ),
        (
            b"M4V ",
            "ISO Media, Apple iTunes Video (.M4V) Video",
            "video/x-m4v",
        ),
        // Two sibling rules match, and both print.
        (
            b"qt  ",
            "ISO Media, Apple QuickTime movie, Apple QuickTime (.MOV/QT)",
            "video/quicktime",
        ),
        (b"jp2 ", "ISO Media, JPEG 2000", "image/jp2"),
        (b"jp2x", "ISO Media, JPEG 2000", "image/jp2"),
        (b"3gp4", "ISO Media, MPEG v4 system, 3GPP", "video/3gpp"),
        (
            b"mp71",
            "ISO Media, MP4 w/ MPEG-7 Metadata [per ISO 14496-12]",
            "application/octet-stream",
        ),
        // file 5.45's magic writes `]b` where it meant `\b`, so this one is
        // printed after a space, with the bracket.
        (
            b"pnvi",
            "ISO Media ]b, Panasonic Video Intercom",
            "application/octet-stream",
        ),
        // Brands it does not know are "ISO Media" and nothing more -- not
        // MP4, which is what this branch used to say for all of them.
        (b"f4v ", "ISO Media", "application/octet-stream"),
        (b"mp45", "ISO Media", "application/octet-stream"),
        (b"xxxx", "ISO Media", "application/octet-stream"),
    ];
    for (brand, desc, mime) in cases {
        check(&ftyp(brand, 200), desc, mime);
    }
}

#[test]
fn a_nested_rule_reads_a_byte_after_the_brand() {
    // 3GPP's release is the brand's fourth byte, as a number.
    check(
        &ftyp(b"3gp\x01", 200),
        "ISO Media, MPEG v4 system, 3GPP, Release 1 (non existent)",
        "video/3gpp",
    );
    check(
        &ftyp(b"3gp\x04", 200),
        "ISO Media, MPEG v4 system, 3GPP, Release 4",
        "video/3gpp",
    );
    // A release the rules do not list prints nothing more.
    check(
        &ftyp(b"3gp\x09", 200),
        "ISO Media, MPEG v4 system, 3GPP",
        "video/3gpp",
    );
    check(
        &ftyp(b"3ge\x07", 200),
        "ISO Media, MPEG v4 system, 3GPP, Release 7 MBMS Extended Presentations",
        "video/3gpp",
    );
    check(
        &ftyp(b"3g2a", 200),
        "ISO Media, MPEG v4 system, 3GPP2 C.S0050-0 V1.0",
        "video/3gpp2",
    );
    check(
        &ftyp(b"3g2b", 200),
        "ISO Media, MPEG v4 system, 3GPP2 C.S0050-0-A V1.0.0",
        "video/3gpp2",
    );
    check(
        &ftyp(b"3g2\x04", 200),
        "ISO Media, MPEG v4 system, 3GPP2 v4 (H.263/AMR GSM 6.10)",
        "video/3gpp2",
    );
}

#[test]
fn sony_xavc_prints_its_codecs_from_far_past_the_brand() {
    let mut b = ftyp(b"XAVC", 200);
    b[96..100].copy_from_slice(b"mp4a");
    b[118..120].copy_from_slice(&48000u16.to_be_bytes());
    b[140..144].copy_from_slice(b"avc1");
    b[168..170].copy_from_slice(&1920u16.to_be_bytes());
    b[170..172].copy_from_slice(&1080u16.to_be_bytes());
    // `beshort` is signed in libmagic, so 48000 Hz prints as -17536.
    check(
        &b,
        "ISO Media, MPEG v4 system, Sony XAVC Codec, Audio \"mp4a\" at -17536Hz, Video \"avc1\" 1920x1080",
        "video/mp4",
    );

    // Unprintable bytes are escaped before `%.4s` takes four characters.
    let mut b = ftyp(b"XAVC", 200);
    b[96..100].copy_from_slice(b"\x01ab\xff");
    b[140..142].copy_from_slice(b"\x7fZ");
    check(
        &b,
        "ISO Media, MPEG v4 system, Sony XAVC Codec, Audio \"\\001\" at 0Hz, Video \"\\177\" 0x0",
        "video/mp4",
    );
}

#[test]
fn a_rule_past_the_end_of_a_short_file_does_not_match() {
    let mut short = 24u32.to_be_bytes().to_vec();
    short.extend_from_slice(b"ftypXAVC");
    check(
        &short,
        "ISO Media, MPEG v4 system, Sony XAVC Codec",
        "video/mp4",
    );
    // 100 bytes: the string at 96 is there (and empty, a NUL), the rest not.
    check(
        &ftyp(b"XAVC", 76),
        "ISO Media, MPEG v4 system, Sony XAVC Codec, Audio \"\"",
        "video/mp4",
    );
    // 96 bytes: the string test still matches at the very end of the file,
    // with nothing to print, as libmagic's does.
    check(
        &ftyp(b"XAVC", 72),
        "ISO Media, MPEG v4 system, Sony XAVC Codec, Audio \"\"",
        "video/mp4",
    );
}

#[test]
fn a_tab_in_a_description_is_printed_escaped() {
    // file 5.45's `caqv` rule has a literal TAB in its text; libmagic's
    // output passes through `file_printable`, which writes it as \011.
    check(
        &ftyp(b"caqv", 200),
        "ISO Media, Casio Digital Camera, Casio Digital Camera\\011Casio",
        "application/octet-stream",
    );
}

#[test]
fn every_rule_in_the_table_is_reachable_and_prints_text() {
    // Guards the generator: a rule with no prefix would match every file,
    // and one whose text is empty would print nothing for its match.
    for rule in crate::isomedia_table::RULES {
        if let super::Test::String(p) | super::Test::StringW(p) = rule.test {
            assert!(!p.is_empty());
        }
        assert!(!rule.text.is_empty(), "a top-level rule with no text");
    }
}
