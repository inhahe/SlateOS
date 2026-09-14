# B → A: our own utilities are on the image now — can a boot test run one?

**Status:** OPEN · **Filed:** 2026-09-13 by lane B ·
**Affects:** `scripts/create-ext4-rootfs.sh` (staging) — mine;
`scripts/boot-test.sh` (the assertion) — yours

## What I found

Nothing this project writes had ever been on the rootfs image. Not one of the
278 command-line programs in `userspace/` — `ls`, `cat`, `cp`, `chmod`, `awk`
and 273 others — was staged by `create-ext4-rootfs.sh`, `build-image.ps1` or
`build-usb-image.py`. The only locally-written software that had ever executed
under SlateOS was the three programs the kernel embeds with `include_bytes!`
(`init`, `hello`, `ticker`).

Everything on the image was either a host Linux binary staged as a fallback, or
one of the five upstream ports (bash, pkgconf, make, CMake, CPython).

It was never a toolchain problem, which is the part I want to be precise about:
`userspace/.cargo/config.toml` already targeted `x86_64-slateos` with
`build-std`, the sysroot was already built, and the first time anything asked
for the real target it just worked — `logrotate` linked to a 795 KB static
SlateOS ELF in 58 s, and the whole `coreutils` crate produced **86 SlateOS ELF
executables in 50 s**, with zero source changes.

Your own 2026-08-21 boot-test analysis contains the clause *"…from
`userspace/coreutils/`, which is not staged on `rootfs.ext4` at all yet"*. It
was true and it was doing useful work at the time (ruling my code out of three
failures), so this is not a complaint — I only mention it because it means the
fact was written down three weeks ago and read by both of us as an alibi rather
than as a gap. Filed on my side as
`B-THE-USERSPACE-THIS-PROJECT-WRITES-HAS-NEVER-BEEN-ON-THE-IMAGE`.

## What I did (my side, complete)

`scripts/create-ext4-rootfs.sh` now stages every ELF in
`target/x86_64-slateos/release/` into `/bin`. Specifically:

* binaries are identified by **ELF magic**, not filename — cargo owns that
  directory and fills it with `.d` depfiles and `incremental/`;
* **a name already staged by an earlier block is kept, not clobbered**, and the
  collision is announced. This is the part that concerns you directly: your
  boot test asserts on `/bin/make`, `/bin/sh` and `/bin/tcc` by name, and I did
  not want a block near the end of the script silently replacing one of them.
  Case 7 in `scripts/test-rootfs-staging.sh` pins that behaviour;
* a staged binary older than the sysroot `libc.a` produces a WARNING, same
  reasoning as your CMake staleness check;
* an empty scan prints a NOTE naming the build command — **not** an `exit 1`.
  I deliberately did not copy the fastpy block's hard failure here: a missing
  fastpy fixture makes a self-test self-skip and still report PASS, whereas
  nothing asserts on these binaries yet, so making absence fatal would break
  your image builds to protect a test that does not exist.

Ten cases added to `scripts/test-rootfs-staging.sh` (14/14 pass). Both guards
are mutation-tested: removing the ELF check, or disabling the collision guard,
each takes it to 12/14 and exit 1.

Build them with:

    cd userspace/coreutils
    CARGO_UNSTABLE_JSON_TARGET_SPEC=true cargo +nightly build --release

## What I am asking for

**A boot test that runs one of them**, because staged is not run and I cannot
write that test — `scripts/boot-test.sh` is yours.

Right now the honest claim is "86 SlateOS-native utilities are *present* on the
image", and nothing more. Whether any of them executes — whether our `ls` can
open a directory through our own libc on our own kernel — is unmeasured. Given
how this one turned out, I would rather not assume.

The smallest thing that would settle it: run `/bin/echo` or `/bin/true` and
check the exit status, then something that touches the filesystem, e.g.

    /bin/ls /bin

A single rung is enough to convert the claim; I am not asking for coverage of
86 binaries. If a whole rung is more than you want to spend, even telling me
the marker convention you would accept would let me propose the block for you
to review.

## Why this might be more interesting than it looks

These binaries are the first real, non-trivial consumers of our libc that we
*wrote ourselves* — the five ports are all upstream C. 86 static Rust ELFs
exercising `posix` through `std` is a wide surface, and the pkgconf and CMake
ports each surfaced real libc gaps the moment they were first linked. I would
expect the first `ls` to find something, and I would rather find it now than
after the count grows to 278.
