## D-CRYPT-HASHES-RUN-AT-THE-LIBCS-SIZE-OPTIMISATION — `crypt`'s hashes took up to 3.7x libxcrypt's time, because the C library is compiled for size (lane D, 2026-10-06) — FIXED 2026-10-06
**Status:** FIXED 2026-10-06 — the hashing in `posix/pwhash`, at opt-level 3 (below)

**In short:** checking a password costs one deliberately expensive hash,
and ours took up to 3.7 times as long as Ubuntu's library takes for the
same hash: 51 ms against 37 for Ubuntu's own yescrypt setting, 5.7 ms
against 1.5 for a smaller one, 9.8 ms against 4.8 for SHA-512 crypt. The
code is not the cause: compiled for speed it matches Ubuntu's (23 ms,
1.6 ms, 5.2 ms). The C library is compiled for size (`opt-level = "s"`,
the workspace's release setting), and hash loops are what that costs most.
Compiling the whole library for speed makes its code 38% larger, so the
fix is a choice, not a one-line change.

**Measured** (2026-10-06, release builds, the same machine, best of three
rounds, ms a hash; `posix/benches/crypt.rs` against
`posix/benches/libxcrypt-reference.c` under WSL):

| setting | ours, opt-level `s` (shipped) | ours, opt-level 3 | libxcrypt 4.4.36 |
|---|---|---|---|
| `$y$j9T$` (yescrypt, 16 MiB: Ubuntu's) | 51.2 | 22.8 | 37.4 |
| `$y$j75$` (yescrypt, 1 MiB) | 5.71 | 1.59 | 1.54 |
| `$7$CU..../....` (scrypt, 64 MiB: libxcrypt's default) | 307 | 184 | 163 |
| `$7$66..../....` (scrypt, 256 KiB) | 1.22 | 0.75 | 0.64 |
| `$6$` (SHA-512 crypt, 5000 rounds: ours) | 9.79 | 5.23 | 4.81 |

The 1 MiB yescrypt row is the honest one: at 16 MiB both libraries wait on
memory, which hides the difference in arithmetic. `performance-targets.md`'s
row asks for 1.5x of libxcrypt; the shipped build misses it everywhere.

**Where:** the root `Cargo.toml`'s `[profile.release]` (`opt-level = "s"`),
which `toolchain/build-sysroot.ps1` builds the libc with; the hashing code
in `posix/src/yescrypt.rs`, `posix/src/sha2.rs`, `posix/src/md5.rs` and
`posix/src/crypt.rs`.

**What fixes it** -- one of:

- **A. The hashes in a crate of their own, at opt-level 3**, as the root
  `Cargo.toml` already compiles the kernel and the media codecs
  (`[profile.release.package.<crate>]`). The speed, and the rest of the
  libc unchanged in size. Costs a crate boundary inside `posix/` and a look
  at what `scripts/check-libc-shape.py` makes of the new archive members.
- **B. The whole libc at opt-level 2 or 3.** At 3 its code grows 38% (the
  host staticlib's `.text`, 1.90 to 2.61 MB), and every program's share of
  it with it; every libc function gets faster. Worth measuring on its own
  -- `printf`, `strtod`, `qsort`, regex, the allocator -- before choosing.
- Not **C, libxcrypt's SSE2 code**: at opt-level 3 the portable code
  already matches it, so the gap is the compiler setting, not the code.

**Fixed (lane D, 2026-10-06) by A.**  The hashing -- SHA-2 and MD5, the
SHA-crypt and md5crypt rounds, yescrypt's KDF -- left `posix` for
`posix/pwhash`, a crate the root `Cargo.toml` compiles at opt-level 3
(`[profile.release.package.pwhash]`, and the dev profile's too, which took
posix's crypt tests in a debug build from minutes to eight seconds); the
rest of the libc stays `-Os`.  `crypt.rs` keeps the settings and the C ABI, and
`yescrypt.rs` the settings and the memory, which `pwhash` cannot allocate.
Two things the move had to get right: nothing hot in `pwhash` is
`#[inline]` or generic across the crate boundary -- either would be compiled
in `posix`, at `-Os` -- so the SHA-crypt core is reached through
non-generic `sha256` and `sha512`; and the first move, the hashes alone,
left SHA-512 crypt at 7.6 ms, its 5000-round loop still `posix`'s, until
that loop moved too.

Measured afterwards, ms a hash, two runs of ours either side of one of
libxcrypt's, with a QEMU boot test running beside them (so treat each as
good to perhaps ±25%):

| setting | ours | libxcrypt 4.4.36 |
|---|---|---|
| `$y$j9T$` | 25.1, 24.4 | 56.7 |
| `$y$j75$` | 0.95, 1.40 | 1.95 |
| `$7$CU..../....` | 201, 210 | 242 |
| `$7$66..../....` | 0.63, 0.58 | 0.77 |
| `$6$` | 3.97, 4.62 | 4.80 |

Ours at or below libxcrypt's time everywhere: the target is met.  That ours
is faster at 16 MiB says more about libxcrypt's memory under WSL's virtual
machine than about either library.
