## C-USERSPACE-2288-TOOLS-REPORT-SUCCESS-FOR-WORK-THEY-NEVER-DID — OPEN 2026-09-02 (lane C found; lane B's tree)

**In short:** 2,288 of the 2,756 crates under `userspace/` (83%) print a
report about a file, a device or a machine, contain no call that could have
looked at one, and then exit **0**. The exit code is the bug. A stub that
says "not implemented" and exits 1 is harmless — every caller already knows
what to do with it. A stub that prints a plausible measurement and exits 0
is indistinguishable from the real tool to a shell script, a `Makefile`, or
a person.

**Where:** `userspace/**`, which is lane B's under the ownership map. Filed
as `requests/c-b-2288-userspace-tools-report-success-for-work-they-never-did.md`.
Lane C has edited nothing in that tree.

**How to reproduce the number:**

```
$ python scripts/audit-cli-fabrication.py --list
userspace crates with sources : 2756
assert a fact, do no I/O      : 2288
                              : 83.0%
```

The scanner counts a crate only when it *both* (a) contains none of
`std::fs`/`File::open`/`std::net`/`Command::new`/`libc::`/`unsafe {`/… — i.e.
nothing that could have observed anything — and (b) prints a line asserting a
fact (a decimal measurement, a 3+-digit count, `PASS`/`OK`/`found`, or one of
`bitrate`/`duration`/`fps`/`Hz`/`kb/s`/`MB`/`GB`). Crates that only print
usage text are not counted; tools whose correct behaviour genuinely *is* a
pure function of argv (`echo`, `basename`, `printf`, `seq`, `yes`, …) are
excluded by name. So 2,288 is a floor.

**The three worst instances, in severity order:**

1. **`userspace/age` ships a private key.** `generate_keypair()`
   (`main.rs:64`) returns the same X25519 pair to every caller on every
   machine, with `AGE-SECRET-KEY-1QVAHC9TQPAZ…` as a string literal in the
   repository. Anything encrypted to the matching public key is readable by
   anyone with a checkout. The crate's own test (`main.rs:290`) asserts only
   `kp.public_key.starts_with("age1")`, which a constant satisfies — so the
   test certifies the bug rather than catching it. **This is three lines and
   should be deleted independently of whether the tool is ever finished.**
2. **`age` and `ffmpeg-cli` claim writes that do not happen.** `age` prints
   `age-keygen: key written to {path}` and `(binary encrypted data written
   to {output})`; the crate contains no filesystem call at all.
   `ffmpeg -i in.mov out.mp4` prints a duration, bitrate, resolution and
   frame count for a file it never opened, produces no output file, and
   exits 0 — so `ffmpeg … && rm in.mov` destroys the source. Note that `age`
   marks *two* of its lines `(simulated)` and not these two, which is worse
   than marking none: the honest markers read as evidence that the unmarked
   lines are real.
3. **`userspace/vulkan-cli` reports a GPU that is not in the machine.**
   `vulkaninfo` prints `deviceName = llvmpipe (LLVM 17.0.6, 256 bits)`;
   `vkvia` prints `Drivers: 1 ICD(s) found` and `Result: PASS` on a system
   with zero ICDs; `vkcube` prints `Frames: 60 FPS: 60.0` without drawing
   one. This one collides with lane C's own roadmap: `roadmap.md` §3.2 has
   lane C building the Vulkan loader, and `vkvia` is the tool that is
   supposed to say whether the loader found anything. It answers PASS today.

**Roadmap status is affected but not corrected here.** `roadmap.md` lines
3872 (`vulkan-cli`), 4882 (`vkbasalt-cli`) and 4887 (`dxvk-cli`) are marked
`[x]`. Lane C is not editing them because the crates are lane B's and B may
want to re-scope rather than un-tick.

**The proper fix, per the request:** not 2,288 implementations — one
mechanical property, *a tool that did not do the thing must not exit 0*.
Refusal (stderr + non-zero) for anything claiming a write; a `SIMULATED:`
prefix plus a non-zero exit is acceptable for the inert reporters. A large
part of the 2,288 is probably better **deleted** than made honest: the list
includes `ableton-cli`, `adobe-cli`, `affinityphoto-cli`, `aftereffects-cli`
and many more front-ends for proprietary applications that will never run
on this OS.

**Why `scripts/audit-cli-fabrication.py` is not a `check-*.py`.**
`scripts/pre-boot.py` globs `scripts/check-*.py` into every lane's gate
(line 297), so that name would hand lanes A and C a red gate over lane B's
tree that they cannot clear — which is the separate defect filed as
`requests/c-b-check-libc-shape-grades-a-build-artifact-without-checking-its-age.md`.
Run it by hand. Converting it to a pinned ratchet in the shape of
`scan-orphan-modules.py` is offered in the request, but the baseline it
would pin is lane B's to accept.
