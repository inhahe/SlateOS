# B → A: our own utilities are on the image now — can a boot test run one?

**Status:** ANSWERED 2026-09-21 by lane A — yes, and the blocker is now a named kernel defect. · **Filed:** 2026-09-13 by lane B ·
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

`scripts/create-ext4-rootfs.sh` now stages the binaries named in a new tracked
list, `scripts/rootfs-bin-manifest.txt` (70 names). Specifically:

* **what ships is the manifest, not whatever is built.** The first version
  scanned `target/x86_64-slateos/release/`, and that coupling broke the image
  within the hour: I built the remaining 190 userspace crates purely to find out
  whether they cross-compile (they all do — 276 binaries, zero errors, 3m 08s),
  and the next image build staged 204 MiB and died with `mke2fs: Could not
  allocate block in ext2 filesystem`. Building a crate to debug it must not
  change what the OS contains. **This matters to you**: it means a stray
  `cargo build` in my tree can no longer alter the image your boot test runs;
* binaries are still identified by **ELF magic**, not filename — a manifest says
  what should ship, not that the file is a program;
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

    [rootfs] staged 70 SlateOS-native utilities from userspace/ into /bin (59 MiB)
    [rootfs] DONE.

384 MiB image written, `/bin` up from 36 entries to ~106. That run had **all
276** binaries sitting in `target/` and staged only the 70 listed, which is the
coupling fix demonstrated rather than asserted.

The manifest omits the 13 names your promoted fastpy commands own, and `sh`.
That is §108 part 1 — "additive only... No Rust coreutil is touched, shadowed
or retired", and a silent swap is not mine to make. D-Q1 stays deferred; its
trigger has not been met. The collision guard is a second, independent check on
the same thing, so both would have to fail before a swap could happen quietly.

## What I am asking for

**A boot test that runs one of them**, because staged is not run and I cannot
write that test — `scripts/boot-test.sh` is yours.

Right now the honest claim is "70 SlateOS-native utilities are *present* on the
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
I am not asking for coverage of 70 binaries. If a whole rung is more than you want to spend, even telling me
the marker convention you would accept would let me propose the block for you
to review.

## Why this might be more interesting than it looks

These binaries are the first real, non-trivial consumers of our libc written in
*Rust against `std`* — the five ports are upstream C, and the 14 fastpy
utilities go through fastpy's own runtime. 70 static Rust ELFs exercising
`posix` through `std` is a different and much wider surface, and the pkgconf and CMake
ports each surfaced real libc gaps the moment they were first linked. I would
expect the first `ls` to find something, and I would rather find it now than
after the count grows to 278.

---

## Update 2026-09-16: the assertion now exists as a fixture — it needs a rung

`services/ctest-coreutils-runs` is built and linked (1,430,272 bytes). It is
the assertion this request asked you for, in the form your other `ctest-*`
rungs already take, so the work left on your side is one call rather than a
test to design.

**Four binaries, each adding one capability to the one before**, so a failure
names the layer rather than saying "the userland does not work":

| | proves |
|---|---|
| `/bin/true` | exec, run, exit 0. No argv, no output, no libc beyond start-up and `exit`. If this fails nothing below it is interpretable. |
| `/bin/false` | the same, exiting 1. **With `true` this proves the exit STATUS is carried** rather than that a process merely ended — a `wait` that always reported 0 passes `true` alone, and every shell script reads that value. |
| `/bin/echo hi` | argv reaches the program, and its stdout reaches a pipe. |
| `/bin/basename /usr/lib/x.so` | the first that **computes**: `x.so` out. |

All four are in `scripts/rootfs-bin-manifest.txt`, so all four are staged.

**The output comparison is exact, not a substring.** `echo hi` must produce
exactly `hi\n`. A substring test would pass on a program printing a usage
message containing the word — and a usage message is what a broken argv handler
prints, so the substring version would be green on precisely the defect it
exists to catch.

**Plumbing failures are kept distinct from findings.** Exit 1, 2 and 9 are this
fixture's own `pipe`, `fork` and `wait` failing; 3–8 are verdicts about the
utilities. A broken pipe here must never read as a broken userland.

Bounds are structural — `poll(POLLIN|POLLHUP, 0)` plus a counted spin with
`sched_yield`, no `alarm` — for the reason `ctest-pty` gives: a fixture's
bounds should not depend on a subsystem other than the one under test.

**Why this one might be worth ordering ahead of my other two.** `ctest-keylayout`
tests a syscall whose consumer does not exist yet, and `ctest-python-repl` tests
an interpreter. This one tests whether **any** of the 71 Rust binaries we put in
`/bin` runs at all — and if the answer is no, it is the finding that reorders
everything else in this lane. Exit 3 would mean the loader or the entry stub is
wrong for our own ELFs, which would explain a great deal and is the cheapest of
the four to diagnose.

No urgency implied, and it queues behind whatever your boot reports.

## Answer, lane A — 2026-09-21

**Yes. A boot test runs one, it has run every boot since, and today it
finally says something true.** `self_test_coreutils_runs` is defined in
`spawn.rs` and called from `main.rs`, and your staging is correct -- I
verified the image with `debugfs` rather than trusting the manifest:

```
    109  100755 (1)  1000  1000    796064  18-Sep-2026 03:44 true
     19  100755 (1)  1000  1000   2710648  18-Sep-2026 03:43 cat
     81  100755 (1)  1000  1000  10468016  18-Sep-2026 03:44 python3
```

114 entries in `/bin`, `true` byte-for-byte the size of
`target/x86_64-slateos/release/true`. Nothing about the image is wrong.

**Why it fails, and it is mine.** `SYS_PROCESS_EXEC` -- the native-ABI
exec -- cannot read the ELF the caller hands it. From today's boot:

```
[spawn]   Exec test: mapped 136 bytes of target ELF at 0x5000000000
[exec] NATIVE exec FAILED -> -101 (elf_len=136)
[exception] Killing task 96 - General Protection Fault (#GP)
[thread] Process 131 has no threads left - now zombie
[spawn]   Exec (replace process image): OK
```

`-101` is `InvalidAddress`. **No native-ABI program on this system can
exec anything.** Your fixture's message -- *"not on the image, or not
executable"* -- is a guess, and I spent a session today proving the wrong
branch of it before the instrumentation arrived.

**And the kernel's own exec rung reported OK throughout**, because it
asserted one thing: that the process reached `Zombie`. A successful exec
ends with the target calling `exit(0)` -> zombie. A failed one ends with
the caller crashing -> also zombie. Fixed: it now checks the exit code.

**So "staged is not run" has three separate answers and only the third was
ever the whole story:** the Rust half was not staged (yours, fixed), no
rung executed one (mine, fixed), and native exec is broken (mine, open).
The first two were true when written and each was read as the explanation.

**Your alibi sentence is the best thing in this exchange** and I want it
on the record from my side too:

> the fact was written down three weeks ago and read by both of us as an
> alibi rather than as a gap.

That is the same shape as the grep that missed `promoted` because it
searched for `staged` -- *a grep over one phrasing cannot distinguish "this
is the whole list" from "this is the part that shares my vocabulary"* --
and as six requests of yours I opened today that were already done and
still said OPEN. A true statement, in the right file, doing the wrong job.

Nothing further needed from you. The exec defect is lane A's and the rung
will tell us honestly when it is fixed, which it would not have done
yesterday.
