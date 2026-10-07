//! How long `crypt` takes for the settings an `/etc/shadow` holds: yescrypt
//! at Ubuntu's cost (`$y$j9T$`, 16 MiB) and at libxcrypt's test cost
//! (`$y$j75$`, 1 MiB), scrypt at libxcrypt's default (`$7$CU`, 64 MiB) and
//! a small one, and SHA-512 crypt at its default 5000 rounds.
//!
//! A password hash is meant to be slow, so "fast" is not the target: the
//! target is libxcrypt's speed (performance-targets.md, "Password hashing").
//! A login here that took several times Ubuntu's would be a worse login,
//! and one an attacker's libxcrypt does several times faster spends the
//! defender's time for nothing.
//!
//! Run with:
//!
//! ```sh
//! cargo bench -p posix --target x86_64-pc-windows-gnu --bench crypt
//! ```
//!
//! and, for libxcrypt's figures on the same machine, `libxcrypt-reference.c`
//! beside this file, under WSL (its header says how).  Each line is the best
//! of three rounds' mean time a hash; the two programs' lines match row for
//! row.
//!
//! Measured on the development machine, 2026-10-06, ms a hash -- with other
//! builds running, so good to perhaps ±20% -- in the bench profile, which is
//! the release profile the libc ships with (`opt-level = "s"`), and again at
//! opt-level 3 (`CARGO_PROFILE_BENCH_OPT_LEVEL=3`):
//!
//! | setting | `s` | 3 | libxcrypt 4.4.36 |
//! |---|---|---|---|
//! | `$y$j9T$` | 51.2 | 22.8 | 37.4 |
//! | `$y$j75$` | 5.71 | 1.59 | 1.54 |
//! | `$7$CU..../....` | 307 | 184 | 163 |
//! | `$7$66..../....` | 1.22 | 0.75 | 0.64 |
//! | `$6$` | 9.79 | 5.23 | 4.81 |
//!
//! The code matches libxcrypt's; the libc's size optimisation does not
//! (known-issues/D-CRYPT-HASHES-RUN-AT-THE-LIBCS-SIZE-OPTIMISATION.md).  The
//! 1 MiB row is the one to watch: at 16 MiB both wait on memory.

use std::hint::black_box;
use std::time::Instant;

fn main() {
    let cases: [(&str, u32); 5] = [
        ("$y$j9T$PKXc3hCOSyMqdaEQArI62/", 20),
        ("$y$j75$LdJMENpBABJJ3hIHjB1Bi.", 100),
        ("$7$CU..../....SodiumChloride", 10),
        ("$7$66..../....SodiumChloride", 200),
        ("$6$saltstring", 200),
    ];
    for (setting, reps) in cases {
        let mut out = posix::crypt::buf();
        // Warm: the first hash pays for the allocator's first mapping.
        let first = posix::crypt::hash_into(b"pleaseletmein", setting.as_bytes(), &mut out);
        assert!(first.is_some(), "{setting} hashes");
        let mut best = f64::INFINITY;
        for _ in 0..3 {
            let started = Instant::now();
            for _ in 0..reps {
                black_box(posix::crypt::hash_into(
                    black_box(b"pleaseletmein"),
                    black_box(setting.as_bytes()),
                    &mut out,
                ));
            }
            best = best.min(started.elapsed().as_secs_f64() / f64::from(reps));
        }
        println!("{setting:<34} {:>9.3} ms", best * 1e3);
    }
}
