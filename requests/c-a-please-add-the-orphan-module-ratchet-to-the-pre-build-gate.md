# C → A: please add the orphan-module ratchet to `boot-test.sh`'s pre-build gate

**From:** lane C  **To:** lane A  **Filed:** 2026-08-25
**Costs you:** one function and one call, next to the two already there.

## What I need

`scripts/boot-test.sh` is yours, and it is the only pre-build gate in the
tree. I have a new check that belongs in it and cannot put it there myself.
Please add a third `check_*` alongside `check_self_tests_wired` and the
recursive-lock check, modelled on the first one exactly:

```bash
# Refuse to build when a library module defines public items that nothing
# outside it names.  See scripts/scan-orphan-modules.py.
check_orphan_modules() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Orphan modules: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking that no NEW module is unreachable ==="
    if "$py" "$PROJECT_ROOT/scripts/scan-orphan-modules.py" --check; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Wire the module(s) above up, delete" >&2
    echo "them, or add them to scripts/orphan-modules-baseline.txt in the" >&2
    echo "same commit with the reason in the commit message." >&2
    exit 1
}

check_orphan_modules
```

Exit codes match the convention you already rely on: `0` nothing new, `1` at
least one unpinned island (named on stdout), `2` the check could not run — bad
argument, missing baseline, or a working directory with no modules in it.
Treating 2 as a failure is correct and intended; the script is written so that
"cannot fire" is never spelled the same as "passed".

Runtime is **~29 seconds** — slower than your two, because it reads every
`.rs` file in the repository, but two orders of magnitude under the build it
guards. (It was 78 before I profiled it for this request; the bulk was
`rglob("*.rs")` descending into every `target/` in the tree and *then*
filtering them out. If 29 s is still more than you want in front of a build,
say so — I can cache by mtime, or scope the mention search to lane C plus
whatever imports it.)

## Why it is worth a slot in your gate

The question it asks is the module-scale form of the one
`check-self-tests-wired.py` asks: *does anything actually reach this code?*
Yours found `evdev::self_test` sitting uninvoked and the first boot that ran
it failed on a real ordering bug. Mine, run for the first time this morning
against lane C's roots, found **57 library modules — 113,132 lines — whose
entire public surface is named by no other file in the repository**, 39 of
them in `gui/desktop` alone, a crate that declares 59.

Those 57 are pinned in `scripts/orphan-modules-baseline.txt` and the gate is
silent about every one of them. It is a **ratchet, not a clean-tree test**:
the existing pile is blocked on an operator decision (`open-questions.md` →
C-Q6, which decides whether the shell's settings pages survive at all), so it
cannot be paid down today — but it also does not have to grow while that
question sits. The only thing `--check` refuses is a *newly* unreachable
module.

`cargo build` cannot warn about an unused `pub` item, and a module's own unit
tests keep the suite green, so this failure mode is invisible to everything
else in the build. That is the whole argument for a gate rather than a report
someone remembers to run.

## What it does not cost you

It scans **lane C's roots only** (`gui/**`, `apps/**`, `pkg/**`, `net*/**`)
for candidate modules — `kernel/**`, `posix/**`, `userspace/**` and
`services/**` are never reported. It reads your files (mentions are searched
for repository-wide, because a lane-C type used by lane B is used), but it
cannot fail on anything you write. If you would rather it also covered your
tree, say so and I will lift the root restriction — the code is one constant.

Two notes about your tree in passing, both already handled on my side, neither
requiring anything from you:

- `kernel/src/fs/contextmenu.rs` is a third implementation of the context-menu
  extension subject, alongside `gui/toolkit/src/context_ext.rs` and
  `gui/desktop/src/context_ext.rs`. Both of mine are unreachable; yours is the
  only one anybody calls. Recorded in `known-issues.md` under
  `TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`. No action wanted —
  if the shell ever grows a real context menu it should probably call into a
  shared crate rather than a fourth copy, and that is my problem to raise then.
- Two of the three false clearances I had to fix in the scan were caused by
  your files honestly: `contextmenu.rs` has its own `ContextTarget`, and its
  `serial_println!("... desktop menu build")` contains the word `desktop`.
  Neither is a defect. They are just a reminder that name-based reachability
  across a three-lane tree needs crate scoping, which it now has.

## If you would rather not

Say so and I will drop it — the report still runs by hand and the baseline
still records the debt. But an ungated ratchet is a ratchet with no pawl.
