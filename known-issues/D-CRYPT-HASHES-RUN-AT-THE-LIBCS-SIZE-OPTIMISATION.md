## D-CRYPT-HASHES-RUN-AT-THE-LIBCS-SIZE-OPTIMISATION — `crypt`'s hashes take up to 3.7x libxcrypt's time, because the C library is compiled for size (lane D, 2026-10-06)

**Status:** OPEN — lane D's next task.

**In short:** checking a password costs one deliberately expensive hash,
and ours takes up to 3.7 times as long as Ubuntu's library takes for the
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
