//! How fast CRC-32 runs, on this machine, over a long input: slicing by 8
//! against the byte-at-a-time loop it replaced.
//!
//! Lane F asked for it (requests/f-a-crc32-reads-a-byte-at-a-time-and-eight-at-
//! a-time-is-five-times-faster.md): every caller checksums whole payloads --
//! a PNG's pixel data, a gzip member, a ZIP entry, every Wi-Fi frame -- and
//! the byte-at-a-time loop waited on each byte before starting the next, about
//! 7 cycles a byte.
//!
//! Run with `cargo bench -p crc32`. The input is 4 MiB of noise, about the
//! size of the PNG pixel data lane F measured. Both loops run in the same
//! process on the same input; the byte-at-a-time one is a copy kept here, the
//! crate's own being private.

/// The byte-at-a-time loop, as `crc32_raw` was until 2026-10-01.
fn bytewise(data: &[u8]) -> u32 {
    let table: [u32; 256] = core::array::from_fn(|i| {
        let mut crc = u32::try_from(i).unwrap_or(0);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
        crc
    });
    let mut crc = !0u32;
    for &byte in data {
        let idx = usize::try_from((crc ^ u32::from(byte)) & 0xFF).unwrap_or(0);
        crc = table.get(idx).copied().unwrap_or(0) ^ (crc >> 8);
    }
    !crc
}

fn main() {
    const LEN: usize = 4 << 20;
    let mut data = vec![0u8; LEN];
    let mut x = 0x9E37_79B9_u32;
    for b in &mut data {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x.to_le_bytes()[0];
    }
    let rounds = 20_u32;

    // Each loop's results are summed and printed, so the optimiser keeps
    // both; the two loops' own results are compared once, outside the timing.
    let started = std::time::Instant::now();
    let mut acc = 0_u32;
    for _ in 0..rounds {
        acc = acc.wrapping_add(crc32::crc32(&data));
    }
    let sliced = started.elapsed();

    let started = std::time::Instant::now();
    let mut acc_old = 0_u32;
    for _ in 0..rounds {
        acc_old = acc_old.wrapping_add(bytewise(&data));
    }
    let old = started.elapsed();
    let (new_crc, old_crc) = (crc32::crc32(&data), bytewise(&data));

    let mib = (LEN as f64) * f64::from(rounds) / f64::from(1_u32 << 20);
    println!("crc32 over {} MiB, {rounds} rounds:", LEN >> 20);
    println!("  slicing by 8:   {:8.1} MiB/s", mib / sliced.as_secs_f64());
    println!("  byte at a time: {:8.1} MiB/s", mib / old.as_secs_f64());
    println!(
        "  speed-up:       {:8.2}x",
        old.as_secs_f64() / sliced.as_secs_f64()
    );
    println!(
        "  checksums agree: {} ({new_crc:#010x}; sums {acc:#010x}, {acc_old:#010x})",
        new_crc == old_crc
    );
}
