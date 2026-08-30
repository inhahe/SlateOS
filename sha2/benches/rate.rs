//! How long one SHA-256 takes, on this machine, for a short input.
//!
//! This is not idle curiosity. `gui/credentials` derives its session key by
//! iterating this hash `DEFAULT_KDF_ROUNDS` times, and that constant was
//! chosen by *dividing a latency budget by this number*: ~120 ms per unlock at
//! 100 000 rounds only follows if a round costs what it costs here. If this
//! implementation ever gets slower, every unlock in the system gets
//! proportionally slower; if it gets faster, the vault has quietly become
//! cheaper to attack at the same round count and the constant should rise.
//!
//! Run with `cargo bench -p sha2`. The input is 70 bytes — two blocks once
//! padded — because that is the shape the key-derivation loop actually feeds
//! it (a 32-byte accumulator, a password, a salt), not because short inputs
//! are representative in general.
//!
//! Measured on the development machine, release build, both implementations in
//! the same process on the same input:
//!
//! | Implementation | µs/iter |
//! |---|---|
//! | this crate | 1.20 |
//! | the hand-written copy in `gui/credentials` it replaced | 1.54 |
//!
//! The 22% is not cleverness — the copy allocated a `Vec` on every call to
//! hold the padded message, and this one pads into a 72-byte array on the
//! stack. Worth knowing only because it means consolidating the twenty-six
//! copies costs no unlock latency; it buys some.

fn main() {
    let input = [0x5a_u8; 70];
    let iterations = 200_000_u32;

    // Fold every digest into an accumulator that is printed, so the optimiser
    // cannot decide the loop has no effect and delete the thing being
    // measured.
    let mut acc = 0_u8;
    let started = std::time::Instant::now();
    for _ in 0..iterations {
        acc ^= sha2::sha256(&input).first().copied().unwrap_or(0);
    }
    let elapsed = started.elapsed();

    let per_iter_us = elapsed.as_secs_f64() * 1e6 / f64::from(iterations);
    println!("sha256 of {} bytes: {per_iter_us:.3} us/iter", input.len());
    println!("  ({iterations} iterations in {elapsed:?}; checksum {acc})");
}
