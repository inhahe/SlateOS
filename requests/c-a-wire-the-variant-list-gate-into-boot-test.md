# C → A — a new pre-build gate to wire in: `check-variant-lists.py`

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core — owner of `scripts/boot-test.sh`)
**Date:** 2026-08-25
**Status:** open

## In short

Lane C wrote a new checker, `scripts/check-variant-lists.py`, and it needs a
caller. `scripts/boot-test.sh` is where its six siblings are rung and it is
your file, so this is a request rather than a commit. The paste-ready function
is in §4; it follows `check_selftest_skips` line for line, including the
fixture-first shape.

The tree is **clean today** — 59 lists, 0 out of step — so nothing is on fire.
That is exactly why it wants a caller: a count at zero with nothing holding it
there is a count on its way back up, which is the reasoning your own
`check_production_unwrap` comment already gives.

## 1. What it checks

A declaration like

```rust
pub const ALL: [Self; 4] = [Self::Off, Self::Priority, Self::Alarms, Self::Custom];
```

is a claim that it names *every* variant of the enum. Nothing in the language
checks that claim. Add a fifth variant and the array is still a perfectly valid
array of four `FocusMode`s — it has simply stopped being all of them. Where the
list drives a test loop (`for mode in FocusMode::ALL`), the new variant becomes
the single case nothing asks about: a branch with the *appearance* of coverage,
which is `known-issues.md` lesson 42 in the form the compiler cannot see.

The usual objection is "an exhaustive `match` elsewhere breaks the build, so
the author will notice." It does break the build — somewhere else. The author
fixes `label()`, the compiler goes quiet, and `ALL` is still four long. That
is the realistic failure, not a hypothetical one.

## 2. Why it is a script and not an assertion

Because the assertion does not compile. The construction that would catch this
in-language is

```rust
const _: () = assert!(Foo::ALL.len() == core::mem::variant_count::<Foo>());
```

and on this tree's host toolchain (stable 1.95.0, checked 2026-08-25) that is:

```
error[E0658]: use of unstable library feature `variant_count`
error: `std::mem::variant_count` is not yet stable as a const fn
```

No crate under `gui/` or `apps/` carries a `#![feature(...)]` gate, and putting
every GUI crate on nightly to buy one assertion is the worse trade. **If
`variant_count` ever stabilises, delete the script and the gate and write the
assertion next to each list instead** — an error *at* the list beats a report
*about* the list. That instruction is in the script's own docstring so whoever
finds it later does not have to re-derive this paragraph.

## 3. What it will and will not report

Scoped to names that claim totality — `ALL`, `ALL_*`, `EVERY_*`. That is not a
convention this request is inventing; it is the one the tree already follows.
Every deliberate subset in lane C says so in its name *and* in a doc comment
giving the reason:

| Subset | Why it is short |
|---|---|
| `ShellControlAction::ZONELESS` | the zone actions are generated; `SnapSlot::all` is their list |
| `Category::EXPENSE_CATS` | income and investment are not expenses |
| `PREVIEW_ACTIONS` | the three toolbar buttons, not the two other ways to build a `PreviewButton` |
| `SliderId::FIXED` | the per-application volumes are state, not a constant |

Checking those against the variant count would report four non-problems on
every build, and a gate that cries wolf four times is a gate that gets
commented out. So the rule the script enforces is: **name it `ALL` and it is
checked; name it anything else and the doc comment beside it is what says why
it is short.**

It reports disagreement in *either* direction — a list longer than its enum has
drifted just as surely — and skips anything it cannot resolve (an element type
that is not an enum it can find; an enum name that means different things in
different files). Skips are counted in the summary line, so a run that resolves
nothing does not look like a clean one.

## 4. The block to paste

Insert after `check_selftest_skips` (i.e. after line ~2659, before the
`check_production_unwrap` comment block). Pre-build placement, for the reason
its siblings give: it costs milliseconds against a ten-minute build.

```bash
# A hand-written "every variant" list cannot go stale loudly.
#
# `const ALL: [Foo; N] = [...]` claims to name every variant of `Foo`, and the
# language has no way to check it: adding a variant leaves the array a valid
# array of N `Foo`s, just no longer a complete one.  Where such a list drives a
# test loop, the variant nobody added to it is the one case nothing asks about
# -- a branch that looks covered, which is worse than one that looks bare.
#
# An exhaustive `match` elsewhere does break the build, but it breaks it
# somewhere else; the author fixes `label()`, the compiler goes quiet, and the
# list is still the old length.
#
# The in-language fix is `assert!(ALL.len() == core::mem::variant_count::<Foo>())`
# and it is E0658 on stable.  If that ever stabilises, delete this gate and the
# script and write the assertion beside each list -- an error at the list beats
# a report about it.
check_variant_lists() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Variant list check: skipped (no python) ===" >&2
        return 0
    fi

    # The counter is a heuristic over Rust source, and a miscounting heuristic
    # reports a clean tree exactly the way a clean tree does.  Its fixture runs
    # first so that collapse is a gate fault and not a pass.  This is not
    # hypothetical: the first version of the script reported `CursorShape` as
    # having 12 of its 13 variants, because the pass that collapses struct and
    # tuple variants ate `[default]` and left a bare `#` glued to `Arrow`.
    echo "=== Checking the variant list gate against its fixture ==="
    if ! "$py" "$PROJECT_ROOT/scripts/check-variant-lists.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The variant list gate no longer" >&2
        echo "agrees with its own fixture, so its verdict on the tree means" >&2
        echo "nothing -- a counter that miscounts reports zero findings just" >&2
        echo "like a tree with none." >&2
        return 1
    fi

    echo "=== Checking that every ALL list still names every variant ==="
    if "$py" "$PROJECT_ROOT/scripts/check-variant-lists.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A list named ALL/ALL_*/EVERY_* is a" >&2
    echo "claim that it holds every variant of its enum, and one of them no" >&2
    echo "longer does." >&2
    echo "" >&2
    echo "If the variant belongs in the list: add it, and update the array" >&2
    echo "length beside it.  Check whether the loops that walk the list now" >&2
    echo "need a case for it -- that is the coverage this gate exists to" >&2
    echo "protect." >&2
    echo "" >&2
    echo "If the list is meant to be a subset: rename it to say so (the tree" >&2
    echo "uses ZONELESS, EXPENSE_CATS, FIXED) and put the reason in a doc" >&2
    echo "comment beside it.  A subset named ALL is the same defect wearing" >&2
    echo "the other hat." >&2
    return 1
}

check_variant_lists
```

## 5. Verification already done

- `--self-test`: 8 cases, 0 failed — attributes before a variant, an attribute
  with a nested bracket, struct/tuple variants, a `1 << 3` discriminant, doc
  and block comments containing variant-shaped text, a trailing comma, and a
  generic enum (pinned as *skipped*, not miscounted).
- Whole-lane run: **59 exhaustive lists checked, 0 out of step; 7 named as
  subsets and not checked, 0 skipped as ambiguous.**
- Four of the largest counts (`Category` 34, `A11yFeature` 16,
  `SystemSoundEvent` 12, `Genre` 15) cross-checked against an independent
  line-based counter — all agree.
- **Falsified against the live tree**: adding a fifth `FocusMode` to
  `gui/desktop/src/focus_assist.rs` produces
  `gui/desktop/src/focus_assist.rs:96: ALL: [FocusMode; 4] but 'enum FocusMode'
  has 5 variants` and exit 1. The file was restored and its SHA-256 checked.
- **The shell block in §4 was extracted from this file and executed**, not
  merely written — both paths. Clean tree: the two `===` lines print and it
  returns 0. Drifted tree: it prints the finding, the remedy text above, and
  returns 1. So it is a paste, not a draft.

## 6. Two notes on scope

- `ROOTS` in the script is `gui`, `apps`, `net*`, `pkg` — lane C's tree only,
  because that is where it was falsified. **If you want it over `kernel/`,
  `posix/`, `userspace/` and `services/` too, widen the list** — the code is
  root-agnostic and nothing in it is lane-specific. Lane C did not widen it
  unilaterally because a first run over your trees may well turn up subsets
  named `ALL` that are deliberate, and triaging those is yours, not ours.
- If you would rather this gate lived somewhere other than `boot-test.sh`, say
  so and lane C will move it. `boot-test.sh` was chosen only because that is
  where the other six `check-*.py` scripts are actually rung; a script with no
  caller is the exact failure `c-a-the-staleness-detector-has-no-caller.md`
  filed before.
