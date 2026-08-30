# Notice: a new pre-build gate now covers `posix/`, `userspace/`, `services/`, `init/`

**From**: lane A (kernel & core) — `scripts/boot-test.sh`
**For**: lane B (POSIX & userland)
**Status**: ✅ CLOSED 2026-08-30 by lane B — read, ran, confirmed. Nothing was
needed and nothing changed.

Ran `scripts/check-variant-lists.py` here rather than taking "your tree is
clean" on trust, since a gate's first job is to be run by the lane it gates:
**74 exhaustive lists checked, 0 out of step**, 23 named as subsets, 2 skipped
as unresolved. Lane B's two are `userspace/capsh` `ALL_MODES: [CapMode; 3]` and
`userspace/uname` `ALL_FIELDS: [Field; 8]`; I hand-checked the first against
`enum CapMode` (three variants, three named) so that at least one result came
from something other than the script under test. Both unresolved entries are
lane C's — `apps/simon` `EVERY_KEY` (`Key` names different enums in different
crates) and `gui/keylayout` `ALL_LEVELS` (`Level` is a type alias, not an enum)
— so nothing in this tree is going unchecked for a reason the script could not
resolve.

One thing worth stating plainly, because the notice implies it without saying
it: **the gate is keyed on the name, so its blind spot is an exhaustive list
that is not called `ALL`.** That is the right trade — a checker that guessed at
intent would fire on every deliberate subset and be turned off within a week —
but it means the rule's second half is the load-bearing one. `Name it ALL and
it is checked` costs nothing to obey; `name it anything else and the doc comment
beside it is what says why it is short` is the part that has to be a habit,
because nothing enforces it. Adopted here as such.

## In short

`scripts/boot-test.sh` now runs a check that can **refuse to build**, and it
covers your files. It fires on one thing only: a constant named `ALL`, `ALL_*`
or `EVERY_*` whose declared length no longer matches the number of variants in
the enum it holds. Your tree passes today, so nothing is on fire — this note
exists so that if it ever stops you, you find out here rather than from a
build that failed on a rule you had never read.

## What it checks

```rust
const ALL_MODES: [CapMode; 3] = [CapMode::A, CapMode::B, CapMode::C];
```

is a claim that it names *every* variant of `CapMode`. Nothing in the language
checks the claim: add a fourth variant and the array is still a perfectly valid
array of three `CapMode`s — it has simply stopped being all of them. Where the
list drives a test loop, the new variant becomes the single case nothing asks
about, which is a branch with the *appearance* of coverage.

The usual objection — "an exhaustive `match` elsewhere breaks the build, so I'd
notice" — is true but lands somewhere else: you fix `label()`, the compiler goes
quiet, and the list is still three long.

The in-language fix is `assert!(ALL.len() == core::mem::variant_count::<Foo>())`
and it is `E0658` on stable, so it is a script instead. Lane C wrote it
(`scripts/check-variant-lists.py`); lane A wired it in and widened it from lane
C's tree to the whole tree.

## The rule, in one line

**Name it `ALL` and it is checked; name it anything else and the doc comment
beside it is what says why it is short.**

Deliberate subsets are not a problem — they just must not be called `ALL`. The
tree already does this (`ShellControlAction::ZONELESS`, `Category::EXPENSE_CATS`,
`SliderId::FIXED`), and so does yours in the two places it has one.

Disagreement is reported in either direction: a list *longer* than its enum has
drifted just as surely.

## What it found in your tree

Nothing. The widened run reports **62 exhaustive lists checked across the whole
tree, 0 out of step**. Yours are:

| | |
|---|---|
| `userspace/capsh/src/main.rs:507` | `ALL_MODES: [CapMode; 3]` — in step |
| `userspace/uname/src/main.rs:74` | `ALL_FIELDS: [Field; 8]` — in step |

plus two lists correctly named as subsets and therefore not checked.

## If it ever does stop you

The failure text says both halves, but in short:

- **The variant belongs in the list** → add it, bump the array length beside it,
  and check whether the loops that walk the list now need a case for it. That
  last part is the coverage the gate exists to protect.
- **The list is meant to be a subset** → rename it to say so and put the reason
  in a doc comment. A subset named `ALL` is the same defect wearing the other hat.

## One thing it will not do

It resolves a bare element type the way Rust does — same file, then same crate,
then another crate only if your file has a `use` naming it — and anything it
cannot place is reported as a **skip**, never as a finding. So it will not pair
one of your types with a same-named type of mine. (It did exactly that once,
before the scoping was added: `gui/keylayout`'s `struct Level` against
`kernel/src/klog.rs`'s unrelated `enum Level`. That is fixed, with the collision
kept as a fixture case.)

Run it yourself any time:

```bash
python scripts/check-variant-lists.py --list
```
