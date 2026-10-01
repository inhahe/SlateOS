//! `swap.c`: Linux swap areas (v0 and v1) and the hibernation images written
//! over them.

use crate::probe::{IdInfo, IdMag, Probe};
use crate::{Bytes, Endianness, USAGE_OTHER};

/// `TOI_MAGIC_STRING`: TuxOnIce.
const TOI_MAGIC_STRING: &[u8] = b"\xed\xc3\x02\xe9\x98\x56\xe5\x0c";
/// `sizeof(struct swap_header_v1_2)`.
const HDR_SIZE: u64 = 516;

/// `!memcmp(mag->magic, LITERAL, mag->len)`: the literal's terminating NUL
/// takes part when the magic is longer than the literal's text.
fn magic_is(magic: &[u8], lit: &[u8]) -> bool {
    let mut l = lit.to_vec();
    l.push(0);
    l.get(..magic.len()) == Some(magic)
}

/// `swap_set_info_swap1`: endianness, page size and size, for SWAPSPACE2.
fn set_info_swap1(pr: &mut Probe, mag: &IdMag, hdr: &[u8]) {
    let e = if hdr.le32(0) == 1 {
        Endianness::Little
    } else {
        Endianness::Big
    };
    pr.set_fsendianness(e);
    let pagesize = mag
        .sboff
        .wrapping_add(u32::try_from(mag.magic.len()).unwrap_or(0));
    pr.set_fsblocksize(pagesize);
    let lastpage = match e {
        Endianness::Little => hdr.le32(4),
        Endianness::Big => hdr.be32(4),
    };
    pr.set_fssize(u64::from(pagesize).wrapping_mul(u64::from(lastpage)));
}

/// `swap_set_info(pr, mag, version)`: the header at 1 KiB.
fn set_info(pr: &mut Probe, mag: &IdMag, version: &[u8]) -> i32 {
    let Some(hdr) = pr.get_buffer(1024, HDR_SIZE) else {
        return pr.none_or_err();
    };
    if version == b"1" {
        // SWAPSPACE2: a version of 1 either way round, and a page count.
        if hdr.le32(0) != 1 && hdr.be32(0) != 1 {
            return 1;
        }
        if hdr.le32(4) == 0 {
            return 1;
        }
        set_info_swap1(pr, mag, &hdr);
    }
    // Garbage in the padding means the label and UUID are not to be trusted.
    if hdr.le32(44 + 32 * 4) == 0 && hdr.le32(44 + 33 * 4) == 0 {
        if hdr.u8_at(28) != 0 && pr.set_label(hdr.span(28, 16)) < 0 {
            return 1;
        }
        if pr.set_uuid(hdr.span(12, 16)) < 0 {
            return 1;
        }
    }
    pr.set_version(version);
    0
}

/// `probe_swap`.
fn probe_swap(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(mag) = mag else {
        return 1;
    };
    // TuxOnIce keeps a valid swap header at the end of the first page.
    let Some(buf) = pr.get_buffer(0, TOI_MAGIC_STRING.len() as u64) else {
        return pr.none_or_err();
    };
    if buf.span(0, TOI_MAGIC_STRING.len()) == TOI_MAGIC_STRING {
        return 1;
    }
    if magic_is(mag.magic, b"SWAP-SPACE") {
        // v0 has no label or UUID.
        pr.set_version(b"0");
        return 0;
    }
    if magic_is(mag.magic, b"SWAPSPACE2") {
        return set_info(pr, mag, b"1");
    }
    1
}

/// `probe_swsuspend`.
fn probe_swsuspend(pr: &mut Probe, mag: Option<&'static IdMag>) -> i32 {
    let Some(mag) = mag else {
        return 1;
    };
    if magic_is(mag.magic, b"S1SUSPEND") {
        return set_info(pr, mag, b"s1suspend");
    }
    if magic_is(mag.magic, b"S2SUSPEND") {
        return set_info(pr, mag, b"s2suspend");
    }
    if magic_is(mag.magic, b"ULSUSPEND") {
        return set_info(pr, mag, b"ulsuspend");
    }
    if mag.magic.get(..TOI_MAGIC_STRING.len()) == Some(TOI_MAGIC_STRING) {
        return set_info(pr, mag, b"tuxonice");
    }
    if magic_is(mag.magic, b"LINHIB0001") {
        return set_info(pr, mag, b"linhib0001");
    }
    1
}

/// A swap magic at the end of a page of each possible size.
macro_rules! at_page_ends {
    ($($m:expr),*) => {
        &[$(
            IdMag::new($m, 0, 0xff6),
        )* $(
            IdMag::new($m, 0, 0x1ff6),
        )* $(
            IdMag::new($m, 0, 0x3ff6),
        )* $(
            IdMag::new($m, 0, 0x7ff6),
        )* $(
            IdMag::new($m, 0, 0xfff6),
        )*]
    };
}

/// `swap_idinfo`.
pub static SWAP: IdInfo = IdInfo {
    name: "swap",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 10 * 4096,
    probefunc: Some(probe_swap),
    magics: at_page_ends!(b"SWAP-SPACE", b"SWAPSPACE2"),
};

/// `swsuspend_idinfo`: TuxOnIce's magic at the start, then the others at
/// each page end.
pub static SWSUSPEND: IdInfo = IdInfo {
    name: "swsuspend",
    usage: USAGE_OTHER,
    flags: 0,
    minsz: 10 * 4096,
    probefunc: Some(probe_swsuspend),
    magics: &[
        IdMag::new(TOI_MAGIC_STRING, 0, 0),
        IdMag::new(b"S1SUSPEND", 0, 0xff6),
        IdMag::new(b"S2SUSPEND", 0, 0xff6),
        IdMag::new(b"ULSUSPEND", 0, 0xff6),
        IdMag::new(b"LINHIB0001", 0, 0xff6),
        IdMag::new(b"S1SUSPEND", 0, 0x1ff6),
        IdMag::new(b"S2SUSPEND", 0, 0x1ff6),
        IdMag::new(b"ULSUSPEND", 0, 0x1ff6),
        IdMag::new(b"LINHIB0001", 0, 0x1ff6),
        IdMag::new(b"S1SUSPEND", 0, 0x3ff6),
        IdMag::new(b"S2SUSPEND", 0, 0x3ff6),
        IdMag::new(b"ULSUSPEND", 0, 0x3ff6),
        IdMag::new(b"LINHIB0001", 0, 0x3ff6),
        IdMag::new(b"S1SUSPEND", 0, 0x7ff6),
        IdMag::new(b"S2SUSPEND", 0, 0x7ff6),
        IdMag::new(b"ULSUSPEND", 0, 0x7ff6),
        IdMag::new(b"LINHIB0001", 0, 0x7ff6),
        IdMag::new(b"S1SUSPEND", 0, 0xfff6),
        IdMag::new(b"S2SUSPEND", 0, 0xfff6),
        IdMag::new(b"ULSUSPEND", 0, 0xfff6),
        IdMag::new(b"LINHIB0001", 0, 0xfff6),
    ],
};
