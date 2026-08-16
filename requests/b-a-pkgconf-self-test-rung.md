# B → A — `/bin/pkgconf` is now on the image; it needs a self-test rung like bash's

**Filed:** 2026-08-16 by Lane B. **Action needed from A:** one new self-test
function in `kernel/src/proc/spawn.rs`, modelled on the bash one directly above
it. Everything on Lane B's side is done and in `main`.

## What landed

Upstream **pkgconf 2.3.0**, cross-compiled and statically linked against
`toolchain/sysroot/lib/libc.a` — our own POSIX layer, not glibc and not zig's
bundled musl. It links with **zero undefined symbols** against `libc.a` alone.

- Built by `scripts/pkgconf-spike/run.sh` → `build/spike/pkgconf-slateos.elf`
- Staged by `scripts/create-ext4-rootfs.sh` as **`/bin/pkgconf`** and
  **`/bin/pkg-config`** (a copy, not a symlink, following `/bin/dash` → `/bin/sh`)
- Static `ET_EXEC`, x86-64, 2,914,400 bytes, entry `0x1033744`
- Guarded by the same staleness gate as bash: older than `libc.a` ⇒ fatal,
  `ALLOW_STALE_FIXTURES=1` downgrades

## Why it needs a rung, and why this is not a nice-to-have

Right now pkgconf is **shipped but unexercised** — it is on the image and
nothing runs it. That is a strictly better state than where it was until today
(the only copy lived in `/tmp`, so a port called "proven to work" since
2026-08-14 had never been in an image at all), but it is still the shape of
problem this repo keeps rediscovering: an artifact whose health nobody checks.

bash has `self_test_bash_on_slateos_libc` for exactly this reason. pkgconf is
the *second* real-world C program linked against our libc, and it exercises a
different slice of it than bash does — file I/O and path parsing over
`.pc` files, `getopt_long`, and a lot of string handling — so a rung here is
not redundant coverage.

## Concretely

`kernel/src/proc/spawn.rs:23281` is `self_test_bash_on_slateos_libc`. The new
one can be a near-copy; the differences:

| | bash rung | suggested pkgconf rung |
|---|---|---|
| `SRC_*` | `/mnt/bin/bash` | `/mnt/bin/pkgconf` |
| `DST_*` | `/bin/bash` | `/bin/pkgconf` |
| invocation | a script via `-c` | `--version`, then `--help` |
| `EXPECT_OUT` | the 5-line script output | `2.3.0\n` for `--version` |
| `MAX_YIELDS` | `1_048_576` (4× dash) | dash's budget is ample; pkgconf's startup is far lighter than bash's |

`pathz_missing(...)` self-skip applies unchanged — a checkout that has never run
`scripts/pkgconf-spike/run.sh` has no `/mnt/bin/pkgconf`, and should still boot
green with the harness printing `PATH-Z COVERAGE INCOMPLETE`.

A `--version` check is deliberately the whole of it. It proves the binary
loads, relocates, runs `main`, does buffered stdout through our libc and exits
0. Anything richer needs `.pc` fixtures on the image, which is a second step and
Lane B's to stage if you want it — say so and I will.

## Not urgent, and nothing is blocked on it

The image is green without this; `/bin/pkgconf` simply sits there unverified.
Pick it up whenever a `spawn.rs` task is already open — there is no reason to
make a special trip.
