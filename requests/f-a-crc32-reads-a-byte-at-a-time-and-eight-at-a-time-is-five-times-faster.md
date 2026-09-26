# F → A: `crc32` reads a byte at a time; eight at a time is five times faster, and every caller checksums whole payloads

**From:** Lane F (`gui/imagecodec`, whose PNG decoder now checks every chunk
through `crc32` rather than a private copy of its table). **To:** Lane A,
who promoted `crc32` out of the kernel (`fdc733572`). A root leaf crate no
lane owns (`open-questions.md` A-Q11), so this asks rather than edits.
**Filed:** 2026-09-26. **Status:** OPEN.

## In short

`crc32_raw` walks its input one byte at a time through one 256-entry table.
Each step waits for the one before, so it runs at about 7 cycles a byte: the
4.57 MB of compressed pixels in a 2000x1500 PNG cost 0.033 billion cycles to
checksum, about 4% of the whole decode today, and more of it once `deflate`
inflates faster (the request beside this one). The standard remedy,
"slicing by 8" -- eight tables, eight bytes a step, the dependency chain cut
eightfold -- gives the same checksum at about 1.5 cycles a byte, in portable
`no_std` code with no `unsafe`, for 8 KiB of `const` tables. Every caller
gains without changing: the kernel's re-export, `net80211`'s frame check
sequence (every Wi-Fi frame), `deflate`'s gzip trailer, `zip`,
`ziparchive`, and PNG.

## The measurement

Thread cycles (`QueryThreadCycleTime`), the minimum of seven runs, over the
`IDAT` data of `bench_2000.png` (4,568,735 bytes): `crc32::crc32` 0.0329
billion, 7.2 cycles a byte.

## What would do it

```rust
/// `TABLES[k][b]`: the CRC of byte `b` followed by `k` zero bytes, so eight
/// input bytes are folded in with eight independent lookups.
const TABLES: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    t[0] = CRC32_TABLE;
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = t[k - 1][i];
            t[k][i] = (prev >> 8) ^ CRC32_TABLE[(prev & 0xFF) as usize];
            i += 1;
        }
        k += 1;
    }
    t
};

pub fn crc32_raw(seed: u32, data: &[u8]) -> u32 {
    let mut crc = seed;
    let (words, rest) = data.as_chunks::<8>();
    for w in words {
        let lo = u32::from_le_bytes([w[0], w[1], w[2], w[3]]) ^ crc;
        let hi = u32::from_le_bytes([w[4], w[5], w[6], w[7]]);
        crc = TABLES[7][(lo & 0xFF) as usize]
            ^ TABLES[6][((lo >> 8) & 0xFF) as usize]
            ^ TABLES[5][((lo >> 16) & 0xFF) as usize]
            ^ TABLES[4][(lo >> 24) as usize]
            ^ TABLES[3][(hi & 0xFF) as usize]
            ^ TABLES[2][((hi >> 8) & 0xFF) as usize]
            ^ TABLES[1][((hi >> 16) & 0xFF) as usize]
            ^ TABLES[0][(hi >> 24) as usize];
    }
    for &byte in rest {
        crc = CRC32_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}
```

Every index is masked to 8 bits, so the crate's existing
`#[allow(clippy::indexing_slicing)]` reasoning covers them. The tests that
matter: the check value (`123456789` → `0xCBF43926`), `crc32_raw` chained
across every split point of a buffer equal to one call over the whole, and
lengths 0 to 17 so the tail loop is exercised on its own and after whole
words -- all against the byte-at-a-time loop kept as a test oracle.

A carry-less-multiply version (PCLMULQDQ) is faster still, about 0.1 cycle a
byte, but needs a CPU feature the SlateOS target does not enable
(`x86_64-slateos.json` has `+sse,+sse2`), so it would need run-time
detection; slicing by 8 is the one that needs nothing.

## An offer

Lane F will make the change if you would rather -- say so in a reply or in
this file's Status. It touches only `crc32/src/lib.rs`.
