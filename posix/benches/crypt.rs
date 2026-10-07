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
//! builds running, so good to perhaps ±25% -- in the bench profile (the
//! release profile the libc ships with): first with the hashing in `posix`
//! at its `opt-level = "s"`, then in `posix/pwhash` at 3, as it ships now
//! (known-issues-resolved/D-CRYPT-HASHES-RUN-AT-THE-LIBCS-SIZE-OPTIMISATION.md),
//! beside libxcrypt's on the same machine at the same time:
//!
//! | setting | in `posix`, `s` | libxcrypt | in `pwhash`, 3 | libxcrypt |
//! |---|---|---|---|---|
//! | `$y$j9T$` | 51.2 | 37.4 | 25.1 | 56.7 |
//! | `$y$j75$` | 5.71 | 1.54 | 0.95 | 1.95 |
//! | `$7$CU..../....` | 307 | 163 | 201 | 242 |
//! | `$7$66..../....` | 1.22 | 0.64 | 0.63 | 0.77 |
//! | `$6$` | 9.79 | 4.81 | 3.97 | 4.80 |
//!
//! Read each against the libxcrypt column beside it: the machine's load
//! moved libxcrypt's own figures by half between the two.  The 1 MiB row is
//! the one to watch: at 16 MiB both wait on memory.

use std::hint::black_box;
use std::time::Instant;

fn main() {
    let cases: [(&str, u32); 7] = [
        ("$y$j9T$PKXc3hCOSyMqdaEQArI62/", 20),
        ("$y$j75$LdJMENpBABJJ3hIHjB1Bi.", 100),
        ("$7$CU..../....SodiumChloride", 10),
        ("$7$66..../....SodiumChloride", 200),
        ("$6$saltstring", 200),
        ("$2b$05$CCCCCCCCCCCCCCCCCCCCC.", 100),
        ("$2b$10$CCCCCCCCCCCCCCCCCCCCC.", 5),
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
