## TD-B-THE-FOUR-SPIKE-ELFS-STILL-MAKE-YOU-RUN-THE-REBUILD-BY-HAND

**Status: FIXED 2026-08-21** — logged and fixed the same day, after being hit a
second time. See the Resolution at the end of this entry.

**In short:** four large programs on the disk image (bash, GNU make, pkgconf,
CPython) are compiled against our own C library. When that library is rebuilt,
those four are out of date, and the image build refuses to run until they are
recompiled. It refuses by *printing the command you should type* rather than
running it — so a purely mechanical fix costs a full build cycle and a human
round-trip. The 70 test fixtures used to work exactly this way and no longer do
(design-decisions.md §355); these four are what is left of the old pattern.

**Where it lives.** `scripts/create-ext4-rootfs.sh`, the four blocks guarded by
`BASH_STALE` / `PKGCONF_STALE` / `MAKE_STALE` / `PYTHON_STALE`. Each prints an
`ERROR:` naming one of:

| artifact | rebuild command | cost |
|---|---|---|
| `build/spike/bash-slateos.elf` | `bash scripts/bash-spike/slatelink.sh` | relink only, seconds |
| `build/spike/pkgconf-slateos.elf` | `bash scripts/pkgconf-spike/run.sh` | configure + build |
| `build/spike/make-slateos.elf` | `bash scripts/make-spike/run.sh` | configure + build |
| `build/spike/python-slateos.elf` | `bash scripts/cpython-spike/slatelink.sh` | relink only, seconds |

Measured 2026-08-21: **all four, end to end, took 83 s** — against an image
build that takes minutes. The cost argument for not doing it automatically does
not survive the measurement.

**How to reproduce.** Rebuild the sysroot (`toolchain/build-sysroot.ps1`, or let
`scripts/ctest-fixtures.py build` do it), then run
`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`. It stages everything,
reaches the bash gate, prints the relink command and exits 1 without writing an
image. This happened while verifying §355 and cost one full cycle for a fix the
script already knew how to perform.

**Why this is the same bug §355 closed, not a separate one.** §355's finding was
that a *gate* has to answer "which side moved?" and cannot, whereas a *build
step* never faces the question because it rebuilds whatever is behind in
dependency order. These four are gates. They are also, unlike the fixtures,
buildable from exactly where the check runs: the spike scripts are WSL bash
scripts and `create-ext4-rootfs.sh` is already running under WSL. The reason the
fixtures are *not* built by the rootfs script — they need Windows-side fastpy and
`zig.exe`, which do not resolve from inside WSL — does not apply here.

**The proper fix.** Have `create-ext4-rootfs.sh` run the rebuild instead of
printing it, in dependency order (sysroot first, then the four), keeping the
`ERROR:` path only for a rebuild that *fails*. Keep an opt-out for the host that
cannot build them, which is what `ALLOW_STALE_FIXTURES=1` already means; consider
a separate `NO_SPIKE_REBUILD=1` so "I know these are stale, pack anyway" stays
distinct from "do not spend the 83 s".

**Why it is not urgent.** The current behaviour is *safe* — it fails closed, and
no stale spike can reach an image. The cost is a wasted cycle per sysroot
rebuild, not a false green. That is why this is debt rather than a bug.

### Resolution — 2026-08-21, lane B

Fixed as described, after the "wasted cycle per sysroot rebuild" was paid a
second time within the hour: the same verification run that logged this entry
hit it again as soon as a `posix/src` edit forced another sysroot rebuild.

`scripts/create-ext4-rootfs.sh` gained a rebuild pass (`spike_rebuild_if_behind`)
that runs *before* the staging blocks, since staging `cp`s each artifact onto
the image tree and only then compares mtimes — rebuilding at the gate would
produce a fresh ELF that never reached the image. For each of the four it
compares against `toolchain/sysroot/lib/libc.a` and, if behind, runs the spike
script, capturing output to `/tmp/spike-rebuild-<name>.log` and printing the
last 20 lines only on failure.

**A missing artifact is deliberately left alone.** Absent is the documented
best-effort case — the self-test self-skips and says so — and a checkout that
has never run a spike should not have a multi-minute configure+build silently
appended to its first image build. Stale is the case that lies, and stale is the
case this fixes.

**Two opt-outs, because they mean different things:**

| variable | meaning | effect |
|---|---|---|
| `NO_SPIKE_REBUILD=1` | "don't spend the 83 s" | skips the pass; the gates below behave exactly as before, so nothing stale ships |
| `ALLOW_STALE_FIXTURES=1` | "I know they're stale, pack anyway" | already downgrades the gates to warnings, so rebuilding first would be wasted work — the pass skips too |

**The four gates stay** as the backstop for a rebuild that was skipped or that
ran and did not clear the staleness. Their messages now say so, so a reader who
reaches one knows the automatic path did not help them and why.

**Only the CPython *interpreter* is rebuilt automatically**, not the stdlib zip:
the interpreter is a libc link, but the zip's staleness is content-based (a
missing `encodings` package, or a deflated member we cannot inflate), which a
relink cannot fix. The gate still names `stdlib.sh` and now explains the split.

**shellcheck caught a live bug in the new code** and is worth noting because the
bug was in an error path nobody exercises. The CPython gate message contained a
literal `` `encodings` `` inside a double-quoted `echo` — which is command
substitution, so it would have tried to run `encodings` and printed "no
package". SC2006. shellcheck was not installed on this machine; it now is, as a
static binary at `~/.local/bin/shellcheck` **inside WSL** (v0.10.0, no admin
required), which is the right place for it since these scripts run under WSL.
`shellcheck -S warning` is clean across `create-ext4-rootfs.sh` and all four
spike scripts; at `-S style` the only remaining note is a pre-existing SC2013 at
line 975.

**Verified** by the run that motivated it: a fresh `libc.a` left all four spikes
behind, and the image build relinked all four and wrote the image without a
human round-trip.
