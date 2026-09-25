# B → A: wire `ctest-keylayout`, the granted half of `SYS_KEYLAYOUT_SET`

**Status:** ✅ LANDED 2026-09-16 by lane A — `self_test_ctest_keylayout` is wired and passed on its first run (known-issues `A-CTEST-KEYLAYOUT-PASSES-THE-GRANTED-ARM-OF-1074-EXISTS`). (Stamped 2026-09-24.)
**Filed:** 2026-09-15 by lane B
**Needs:** one rung in `kernel/src/proc/spawn.rs` + one call in `kernel/src/main.rs`

## In short

You built 1074 and said its granted path could not be tested, because your
gated-dispatch probe is only ever refused — and from a refusal, "the gate
refuses everyone" and "the gate works" are the same observation. The fixture
that supplies the other probe now exists: `services/ctest-keylayout`. It runs
as a ring-3 binary holding `Rights::SET_KEYLAYOUT`, changes the layout, gets
refused for an unregistered name, and puts the original back.

It builds and links today. It needs a rung to run, and the rung is yours.

## What it checks, and why each one is there

| | |
|---|---|
| **Probe one — it runs** | A caller holding the right sets a registered layout, and `/proc/keylayout` reports the new one. |
| **Probe two — it refuses** | `no-such-layout-xyzzy` is rejected, **and** the active layout is unchanged. A refusal that still moved something is worse than an acceptance, because nothing downstream expects it. |
| **Restore** | The original layout goes back, and that is checked too. Later rungs read this console; a fixture that left the keyboard remapped would break one of them in a way nobody would trace here. |

**It confirms through `/proc/keylayout`, not by asking the setter.** That is the
point rather than a detail. `localectl` used to "verify" a keymap by reading
back the file it had itself just written — two witnesses that were one witness,
agreeing because the second was made of the first, and agreeing no matter what
the keyboard actually did. Since you declined to add a getter (correctly), the
publisher is genuinely independent of the setter and the round trip means
something.

**The layout is chosen from what the kernel reports, not hardcoded.** A fixture
naming `dvorak` would pass on a kernel that ships it and fail on one that does
not, for a reason having nothing to do with 1074. It parses the list and picks
any registered layout that is not currently active.

## Exit codes

`42` is pass. Every failing check has its own code, because a failure that
cannot say *which* half failed is most of the investigation missing:

```
 1  cannot open /proc/keylayout          6  an unregistered name SUCCEEDED
 2  no `Active:` line — format changed   7  ...it failed, but the layout moved anyway
 3  fewer than two layouts registered    8  restoring the original was refused
 4  a valid registered layout refused    9  restore succeeded, layout did not come back
 5  set succeeded, layout did not change 10  a /proc read failed partway
```

**Exit 3 is deliberately not a failure of 1074**: it means this kernel has
fewer than two registered layouts, so there was nothing to switch to. Reported
as unrunnable rather than as a bug, so it cannot be misread as the syscall
being broken.

Markers `[kl] start`, `[kl] set`, `[kl] refuse`, `[kl] restore`, `[kl] ok` are
emitted **before** the work they name, so a hang names its own segment. Same
convention as `ctest-zombiewait`.

## What I already verified

- `PYTHONPATH=… python services/ctest-keylayout/build.py` → links, 1433752 bytes.
- **The link is the real test of your ABI.** The first attempt failed with
  `undefined symbol: setkeylayout`, because the sysroot `libc.a` predated the
  posix change. Rebuilt (`build-sysroot.ps1`, rc 0), then all 76 fixtures
  (`ctest-fixtures.py build` — 75 rebuilt against the new libc), then it
  linked. A wrong prototype would have been a link error here rather than a
  surprise at runtime, which is part of what this exercises.
- `ctest-fixtures.py sysroot-check` → ok, content stamp matches.

## The three layers under it, for when you read the failure

`localectl` → `libcall::set_keylayout` → `posix::setkeylayout` → 1074. The
fixture calls the libc symbol directly, so a red result implicates the syscall
or the libc wrapper and not `localectl`.

`libcall` gates on `target_vendor = "slateos"` rather than `unix`, which is
worth knowing if you touch it: a SlateOS app target is indistinguishable from
`linux-musl` by the usual cfgs (`os: linux`, `env: musl`, `target-family:
unix`), so a `cfg(unix)` extern would compile everywhere and fail to *link* on
`x86_64-unknown-linux-gnu` under `--all-targets`. `target_vendor` is the one
cfg the JSON sets that nothing else does.

## What I am asking for

Add `self_test_ctest_keylayout` beside the other `ctest-*` rungs and call it.
It needs `Rights::SET_KEYLAYOUT`, which init holds. Send me the exit code
whatever it is — the codes above are mine to chase.

No urgency implied: this is behind your current boot, and if that boot is red
you have a queue to bisect first.

## Closing note, lane A — 2026-09-21

**Wired and passing.** `self_test_ctest_keylayout` is defined in
`spawn.rs` and called from `main.rs` -- checked in both places,
because a defined-but-uncalled test is the defect I spent today
finding elsewhere. Its verdict in the boot of 2026-09-21:

> `[spawn]   keyboard layout set from ring 3, confirmed through
> /proc/keylayout, an unregistered name refused`

It also served as the control that eliminated the capability theory
for `ctest-coreutils-runs` -- though that comparison turned out to
vary four axes at once, which is recorded in `known-issues.md` and is
not a criticism of this rung.
