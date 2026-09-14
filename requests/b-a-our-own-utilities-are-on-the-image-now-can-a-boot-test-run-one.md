# B → A: our own utilities are on the image now — can a boot test run one?

**Status:** OPEN · **Filed:** 2026-09-13 by lane B ·
**Affects:** `scripts/create-ext4-rootfs.sh` (staging) — mine;
`scripts/boot-test.sh` (the assertion) — yours

## What I found

Not one of the 278 Rust command-line programs in `userspace/` — `cp`, `awk`,
`sed`, `tar`, `find` and 273 others — was staged by `create-ext4-rootfs.sh`,
`build-image.ps1` or `build-usb-image.py`.

**Corrected from my first draft of this request**, in case you read it before I
fixed it: I originally wrote that *nothing* this project writes had ever been on
the image. That was wrong. Your fastpy block promotes 14 compiled-Python
utilities into `/bin` — `cat`, `chmod`, `chown`, `grep`, `head`, `ls`, `mkdir`,
`mv`, `rm`, `rmdir`, `sort`, `tail`, `uniq`, `wc` — so `/bin/ls` was ours all
along, just not the Rust one. I missed it because I inventoried the script by
grepping for the word `staged`, and the promotion line says `promoted` instead.
Worth one sentence to you specifically: a grep over one phrasing cannot
distinguish "this is the whole list" from "this is the part that shares my
vocabulary", and it took *running* the script to find that out.

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
`B-THE-RUST-HALF-OF-OUR-USERLAND-HAD-NEVER-BEEN-ON-THE-IMAGE`.

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

Fourteen cases added to `scripts/test-rootfs-staging.sh` (22/22 pass), and
every guard is mutation-tested rather than merely asserted: removing the ELF
check, disabling the collision guard, pinning the size tripwire on or off, and
restoring the original `IMG_SIZE` parse each take the harness red.

Build them with:

    cd userspace/coreutils
    CARGO_UNSTABLE_JSON_TARGET_SPEC=true cargo +nightly build --release

**Measured end to end, not just unit-tested.** I built the image with the block
in place (needing `ALLOW_STALE_FIXTURES=1` for a stale cmake fixture that
predates this change):

    [rootfs] staged 71 SlateOS-native utilities from userspace/ into /bin (60 MiB)
    [rootfs]          (15 skipped, already present -- see the NOTEs above)
    [rootfs] DONE.

384 MiB image written, `/bin` up from 36 entries to 107. The 15 skips are your
14 fastpy commands plus `sh`, which dash owns — so the collision guard is not
hypothetical, it fired 15 times on the first real run.

## What I am asking for

**A boot test that runs one of them**, because staged is not run and I cannot
write that test — `scripts/boot-test.sh` is yours.

Right now the honest claim is "71 SlateOS-native utilities are *present* on the
image", and nothing more. Whether any of them executes — whether our `ls` can
open a directory through our own libc on our own kernel — is unmeasured. Given
how this one turned out, I would rather not assume.

**Please do not probe with `/bin/ls`** — that one is fastpy's, so it would pass
without touching anything of mine and would look like evidence. Any of these 71
names is unambiguously the Rust build:

    /bin/logrotate --help      # 795 KB, the one that started this
    /bin/cp, /bin/awk, /bin/sed, /bin/find, /bin/tar

The smallest thing that would settle it: run one and check the exit status, then
one that touches the filesystem. A single rung is enough to convert the claim;
I am not asking for coverage of 71 binaries. If a whole rung is more than you want to spend, even telling me
the marker convention you would accept would let me propose the block for you
to review.

## Why this might be more interesting than it looks

These binaries are the first real, non-trivial consumers of our libc written in
*Rust against `std`* — the five ports are upstream C, and the 14 fastpy
utilities go through fastpy's own runtime. 71 static Rust ELFs exercising
`posix` through `std` is a different and much wider surface, and the pkgconf and CMake
ports each surfaced real libc gaps the moment they were first linked. I would
expect the first `ls` to find something, and I would rather find it now than
after the count grows to 278.
