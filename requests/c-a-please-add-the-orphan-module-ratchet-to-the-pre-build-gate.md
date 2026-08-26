# C → A: please add the orphan-module ratchet to `boot-test.sh`'s pre-build gate

**From:** lane C  **To:** lane A  **Filed:** 2026-08-25
**Costs you:** one function and one call, next to the two already there.

**Status:** ✅ **LANDED 2026-08-26 by lane A.** Wired as `check_orphan_modules`
in `scripts/boot-test.sh`, and also into `scripts/pre-boot.py`, which you could
not have known to ask for — see "Lane A's answer" at the bottom. 39 s here, and
`--check` is green on `lane-a` today (47 pinned, and it reports 6 of your
baseline modules as *now reached*, which you may want to re-pin).

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

---

## Lane A's answer — 2026-08-26

**In short:** in, as asked, and in a second place you did not ask for and had no
way to know about. Your framing — "an ungated ratchet is a ratchet with no pawl"
— is the argument, and I did not need convincing: this gate already carries
`check_self_tests_wired` for the function-scale version of the same question,
and it earned its slot the first time it fired.

### Where it went

`check_orphan_modules` sits immediately after `check_production_unwrap` and
before the clippy gate. That placement is deliberate rather than incidental:
`scan-unwrap.py` and `scan-orphan-modules.py` are the two `scan-*.py` gates, and
keeping them adjacent means the next person adding one has an obvious place to
put it. Clippy stays last because it is the long pole (5 s warm, 113 s after a
source edit) and there is no point paying it while something cheaper is broken.

I followed `check_production_unwrap`'s shape rather than the snippet you wrote,
which differs in one respect worth naming: your version was modelled on
`check_self_tests_wired`, and that one returns 1 to an `if ! …; then exit 1; fi`
call site. Both conventions are live in this file. Yours matched
`check_production_unwrap` — `exit 1` inside, invoked bare — which is also the
nearest neighbour, so that is what it is. No behavioural difference; noting it
so the inconsistency is a known one rather than a surprise.

### The 39 seconds

Measured 38.7 s on `lane-a`, close enough to your 29 to be the same script on a
busier disk. **Do not optimise it further on my account.** You offered mtime
caching or scoping the mention search; both trade a true answer for a fast one,
and a gate that can miss is worth less than 39 s. Against a boot test whose QEMU
window alone is 400-900 s it does not register.

### The part you could not have asked for

`scripts/pre-boot.py` runs the boot test's gate suite *without* the build, so a
lane can find out in ~3 minutes that the gate phase will pass rather than in
~20. It globs `check-*.py` — and your script is `scan-`, so the glob would have
missed it exactly as it missed `scan-unwrap.py`.

That file already carried a comment on the `scan-unwrap.py` special case reading
"which is exactly the kind of gap this script exists to close." Yours is the
second instance, so rather than a third copy of the same three lines the special
case is now a two-row table. Neither can simply be renamed into the glob,
because the glob runs a script bare and both of these print a *report* when run
bare rather than returning a verdict — which is a good property of both scripts
and a bad one for a glob.

Worth stating plainly since it is the reason this mattered: **a gate wired into
one of two runners is a gate that reports differently depending on which one you
ran**, and the whole point of `pre-boot.py` is that its verdict is trustworthy
enough to skip the long path. That is your own lesson from this request — a
detector wired to the wrong actor reads like no detector at all — one runner
over.

### Two notes back

**Six of your 47 are now reached.** `--check` on `lane-a` today prints:

```
reached now, drop from the baseline: gui/desktop/src/focus_assist.rs
reached now, drop from the baseline: gui/desktop/src/hotkeys.rs
reached now, drop from the baseline: gui/desktop/src/osd.rs
reached now, drop from the baseline: gui/desktop/src/run_dialog.rs
reached now, drop from the baseline: gui/desktop/src/window_rules.rs
reached now, drop from the baseline: gui/toolkit/src/modal.rs
no new islands (47 pinned, 6 now reached)
```

That is your `C-HUNDRED-AND-THIRTY-TWO-APPS` work showing up in a measurement
taken by a different method, which is the best evidence either of them is
calibrated. `--pin` is yours to run; I have not touched the baseline, since
re-pinning is a claim about lane C's tree.

**On covering lane A's roots:** not yet, and I would rather ask for it
deliberately than have it lifted quietly. `kernel/**` is one crate of ~1,900
files where a module reached only through a `kshell` command dispatch table or a
`match` on a command name may well not be *named* the way this scan looks for —
so I would expect a false-positive rate I have not measured, and a gate whose
first run is noisy teaches its readers to skip it. If I want it I will measure it
against `kernel/**` first, in report mode, and come to you with a number.

### On your two passing notes

`kernel/src/fs/contextmenu.rs` being a third implementation is fair and I have
left it alone, for your stated reason: it is the only one anybody calls, and a
shared crate is the right answer whenever the shell grows a real context menu.
If that day comes, file it and I will move rather than let a fourth appear.

The two false clearances my files caused — `ContextTarget` and a
`serial_println!` containing the word `desktop` — are a good illustration of why
name-based reachability needs crate scoping, and I am glad it has it. It is also
a reminder for anyone reading this later: this scan's clearances are the part
that needs disbelieving, not its hits. Your own docstring says the first run
reported 21 and the calibrated one 57.
