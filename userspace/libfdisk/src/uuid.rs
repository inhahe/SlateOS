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

/// `ul_random_get_bytes (buf, nbytes)`: `buf` filled from the kernel --
/// `getrandom`, else `/dev/urandom`, else `/dev/random` -- and then, always,
/// mixed with a generator seeded from the process and the time, as upstream
/// mixes `rand ()` in: "the only source of randomness if
/// /dev/random/urandom is out to lunch", so that no two disks get the same
/// identifier even then.
pub fn random_get_bytes(buf: &mut [u8]) {
    use randrange::RandomSource;
    if randrange::fill_secret(buf).is_err() {
        for path in ["/dev/urandom", "/dev/random"] {
            if let Ok(mut f) = std::fs::File::open(path)
                && f.read_exact(buf).is_ok()
            {
                break;
            }
        }
    }
    // `crank_random`'s seed: the process, and the time to the microsecond.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seed =
        (u64::from(std::process::id()) << 16) ^ now.as_secs() ^ u64::from(now.subsec_micros());
    let mut rng = randrange::SeededRng::new(seed);
    for b in buf.iter_mut() {
        // `(rand () >> 7) & 0xFF`.
        *b ^= u8::try_from((rng.next_u32() >> 7) & 0xFF).unwrap_or(0);
    }
}

/// `uuid_generate_random (out)`: a version-4 UUID from
/// [`random_get_bytes`].
#[must_use]
pub fn generate_random() -> Uuid {
    let mut buf = [0u8; 16];
    random_get_bytes(&mut buf);
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
