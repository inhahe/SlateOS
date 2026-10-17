//! The parts of libuuid libfdisk uses: `uuid_parse`, `uuid_unparse_upper`,
//! `uuid_is_null` and `uuid_generate_random`, in libuuid's layout -- the
//! sixteen bytes in the order their hex pairs are written. (GPT's on-disk
//! mixed-endian layout is `gpt`'s business.)

use std::io::Read;

/// `uuid_t`.
pub type Uuid = [u8; 16];

/// `uuid_parse (in, uu)`: `Some` for exactly 36 characters,
/// `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` in hex of either case.
#[must_use]
pub fn parse(s: &[u8]) -> Option<Uuid> {
    if s.len() != 36 {
        return None;
    }
    for (i, &c) in s.iter().enumerate() {
        let dash = matches!(i, 8 | 13 | 18 | 23);
        if dash != (c == b'-') || (!dash && !c.is_ascii_hexdigit()) {
            return None;
        }
    }
    let hex: Vec<u8> = s.iter().copied().filter(|&c| c != b'-').collect();
    let mut out = [0u8; 16];
    for (o, pair) in out.iter_mut().zip(hex.chunks(2)) {
        let h = |c: u8| match c {
            b'0'..=b'9' => c.wrapping_sub(b'0'),
            b'a'..=b'f' => c.wrapping_sub(b'a').wrapping_add(10),
            _ => c.wrapping_sub(b'A').wrapping_add(10),
        };
        let (hi, lo) = (
            pair.first().copied().unwrap_or(b'0'),
            pair.get(1).copied().unwrap_or(b'0'),
        );
        *o = (h(hi) << 4) | h(lo);
    }
    Some(out)
}

/// `uuid_unparse_upper (uu, out)`.
#[must_use]
pub fn unparse_upper(uu: &Uuid) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(36);
    for (i, &b) in uu.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push(b'-');
        }
        out.push(HEX.get(usize::from(b >> 4)).copied().unwrap_or(b'0'));
        out.push(HEX.get(usize::from(b & 15)).copied().unwrap_or(b'0'));
    }
    out
}

/// `uuid_is_null (uu)`.
#[must_use]
pub fn is_null(uu: &Uuid) -> bool {
    uu.iter().all(|&b| b == 0)
}

/// `uuid_generate_random (out)`: a version-4 UUID from the kernel's random
/// bytes. Upstream generates one even when the randomness fails (from the
/// bytes it has); so does this, from whatever was read.
#[must_use]
pub fn generate_random() -> Uuid {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    // uu.time_hi_and_version = (... & 0x0FFF) | 0x4000.
    if let Some(b) = buf.get_mut(6) {
        *b = (*b & 0x0F) | 0x40;
    }
    // uu.clock_seq = (... & 0x3FFF) | 0x8000.
    if let Some(b) = buf.get_mut(8) {
        *b = (*b & 0x3F) | 0x80;
    }
    buf
}
