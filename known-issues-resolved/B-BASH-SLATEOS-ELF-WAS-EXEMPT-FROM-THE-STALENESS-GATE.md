### B-BASH-SLATEOS-ELF-WAS-EXEMPT-FROM-THE-STALENESS-GATE. The one artifact that exercises our libc hardest was the one artifact allowed to be out of date — 2026-08-16 — ✅ RESOLVED 2026-08-16 by lane B (gate in `scripts/create-ext4-rootfs.sh`; stale binary relinked; boot-verified green on `main`)

**In short:** The image build refuses to ship a ring-3 test program that is
older than the C library it was compiled against, because such a program tests
the *old* library while reporting a pass about the new one. That rule was
written for the nine small `ctest-*` programs and never applied to a tenth,
much bigger one: GNU bash. Bash was found four days out of date the day anyone
checked, which means the boot test had been reporting "bash works on our libc"
about an Aug-12 libc on every boot since. The rule now covers bash too. The
stale binary itself has not been relinked yet — that needs the cross-build
objects, not a one-line command.

**Why bash is the worst possible exemption.** Every other staged real-world
binary (dash, make, tcc) is a stock Ubuntu glibc program that SlateOS runs
through the staged glibc — it exercises the *loader*, not our libc. Bash is the
only large program compiled from source *against* `toolchain/sysroot/lib/libc.a`,
and at ~5.3 MB it is roughly double any `ctest-*` fixture. It references 2,030
libc symbols where a fixture references a few dozen. So it was simultaneously
the broadest test of the POSIX layer and the only one permitted to be testing a
library that is no longer in the build — the widest false-green on the image.

**Why it was missed.** The artifact is built by `scripts/bash-spike/` into the
**gitignored** `build/spike/`, and `slatelink.sh` hardcodes the *integration*
worktree path (`/mnt/d/visual studio projects/os/build/spike/…`). So it exists
in exactly one of the four worktrees. In the three lane worktrees the file is
simply absent, the self-test self-skips, the harness prints `PATH-Z COVERAGE
INCOMPLETE`, and nothing looks wrong; in `os` — the tree `main` is built from —
it is present and was silently ageing. A per-worktree artifact behind a
best-effort skip is invisible from either side.

**Where it lives.** `scripts/create-ext4-rootfs.sh`, the `BASH_SLATE` block
(~842) and the new `BASH_STALE` gate after the `ctest-*` gate (~941). The
consumer is `kernel/src/proc/spawn.rs::self_test_bash_on_slateos_libc`.

**Fix applied.** Absent and stale are now deliberately *not* treated alike:

| State | Before | Now | Why |
|---|---|---|---|
| absent | warning, boot green | warning, boot green | A skip reports nothing **and says so**; the harness already prints `PATH-Z COVERAGE INCOMPLETE`. Unlike a fixture it cannot be rebuilt from a one-line command, so failing a fresh checkout would be punitive. |
| present, older than `libc.a` | staged silently | **exit 1** | A stale binary reports OK and is wrong. This is the `ctest-*` rule verbatim; `ALLOW_STALE_FIXTURES=1` downgrades it, since a host that cannot rebuild the fixtures certainly cannot relink bash. |

**~~Still open~~ — CLOSED 2026-08-16, same day, and the relink proved the point.**
`os/build/spike/bash-slateos.elf` was dated 2026-08-12 and had to be relinked
against the current sysroot before any image built from `os` would pass the new
gate. Done: `wsl -d Ubuntu -- bash scripts/bash-spike/slatelink.sh` exited 0 with
**zero undefined symbols**, and the artifact went from 5,349,720 to 5,398,808
bytes — **+49,088 bytes of libc that the shipped binary did not previously
contain.** That size delta is the evidence the entry was arguing for in the
abstract: the library really had moved underneath it, so every boot from `main`
between 08-12 and today reported "bash on our libc: OK" about a libc four days
old. `os`'s nine `ctest-*` fixtures were rebuilt alongside it (they were 08-14,
and its `ctest-jobctl.elf` predated the 33 new `waitid` checks), and the rootfs
rebuilt clean — zero staleness warnings, bash staged, 9 fixtures staged.

Boot-verified on `main` the same day: BOOT_OK in 291 s, with
`GNU bash 5.2 on our own libc.a (ring 3 …): OK` — this time about the *current*
libc — and no `PATH-Z COVERAGE INCOMPLETE` line, which is what the previous
run's log had flagged and what led here in the first place.

**Worth generalising, again.** This is the third instance of the pattern
`B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` names: the check was not wrong, it just
did not cover everything it applied to. When a rule is written for a *set* of
artifacts, enumerate the set from the thing they have in common — here "links
`libc.a`" — not from the directory that happened to hold them when the rule was
written (`services/ctest-*`).
