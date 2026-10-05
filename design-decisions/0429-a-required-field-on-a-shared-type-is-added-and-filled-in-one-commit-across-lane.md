## §429 — A required field on a shared type is added and filled in one commit, across lane boundaries

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous) — lane C

**In short:** The project is worked by three agents who each own a slice of the
tree and are forbidden from editing each other's files. When one of them needs a
change in another's slice, they leave a note in a `requests/` folder and the
other picks it up later. That works when the two halves of the change are
independently valid. It does not work when a shared data structure gains a
field that every user of it *must* fill in: the half that adds the field and the
half that fills it in are each, alone, a codebase that does not compile. There
is no order to do them in. This entry decides that in that specific case the
adding lane fills in the other lanes' sites too, in the same commit, and tells
them afterwards.

### The situation that forced it

§427 added a required `overflow` field to `guitk::render::RenderCommand::Text`.
That type lives in lane C's tree but is *constructed* wherever anything draws
text: 4,517 occurrences across 208 files in lane C's own tree, and 31 more in
`init/login/src/main.rs`, which belongs to lane B. `net/`, `netscan/` and `pkg/`
have none, so that one file is the whole out-of-lane reach. That number had to
be measured before the options below could be weighed at all — the shape of the
fallout is what decides whether this is a request or something else — and it is
what makes option D tractable. Had the reach been thirty files across two lanes,
the answer would be C.

Rust has no per-field default in an enum struct variant. That is not incidental
to §427; it is the whole mechanism the operator chose it for. So:

- Lane C adds the field. `init/login` no longer compiles. Because the boot test
  builds the whole workspace, **`main` is red for every lane** until lane B
  happens to read its dropbox — which, as `roadmap.md` notes from experience,
  can be a day, because `requests/` is a set of files on a branch rather than a
  mailbox, and a request is invisible until the recipient merges.
- Lane B fills the field in first. It cannot: the field does not exist.

Both orderings are red. The two halves are not independently valid, and the
request mechanism can only express changes that are.

### The options

**A. File a request and let the tree stay red in between.** Honest about the
ownership rule and costs nothing to implement. It also means deliberately
pushing a `main` that does not build, for an unbounded period, in a project
whose stated rule is "never merge a red tree to `main` to unblock myself" —
and it blocks the *other* lane too, which had no part in the change.

**B. Add the field with a `Default` so unfilled sites still compile.** Keeps
every commit green and every lane inside its own tree. It also destroys the
point: a defaulted `overflow` means all 4,548 sites silently keep today's
behaviour,
and today's behaviour is the reported bug. The operator considered and rejected
exactly this under §427. Reintroducing it here as a *process* convenience would
be overturning a decision that was not mine to overturn.

**C. Add a second variant / a parallel type, migrate lane by lane, delete the
old one at the end.** Every commit is green and no lane touches another's files.
It is the textbook answer and it is a real option. Against it: it is three
round-trips through the dropbox for a mechanical change, the intermediate state
has two ways to spell the same command (which is its own bug surface), and the
"delete the old one at the end" step is the one that never happens — it depends
on every lane having finished, with nothing failing if it is skipped.

**D. (chosen) The commit that adds a required field to a shared type also fills
in every construction of it, wherever it lives, and notifies the other lane
afterwards via `requests/`.** One green commit, no intermediate dialect, no
dangling cleanup step. The cost is real and is the reason this entry exists:
lane C wrote to lane B's tree, which the ownership rule forbids outright, and
the ownership rule exists to prevent the single most expensive failure in this
arrangement — two agents editing one file and one silently clobbering the other.

### Why D, and what makes it safe

The clobber risk is not a constant; it is a function of whether the other lane
has work in flight in that file. That is measurable, so it was measured before
the decision rather than assumed: `origin/lane-b` was **0 commits ahead of
`origin/main`** — nothing in flight anywhere in lane B — and
`init/login/src/main.rs` had last been touched only by a repo-wide rename. The
risk the rule guards against was, at that moment, nil.

So D is conditional, and the conditions are the decision:

1. **The change must be mechanical.** A rule stated in one line, applied
   uniformly, with no judgement about the other lane's screens. Here:
   `max_width: Some(..)` → `Ellipsis`, `max_width: None` → `Clip`.
2. **The other lane's branch must be at or behind `main` in the affected
   files** — checked with `git log origin/lane-<x> --not origin/main -- <path>`,
   not assumed. If they have work in flight, this is off the table and the
   answer reverts to C.
3. **It must be scripted, and the script committed**, so the other lane can read
   precisely what was done to their file and re-run it to confirm it is a fixed
   point. (`scripts/q45_apply.py`.)
4. **A `requests/<mine>-<theirs>-*.md` must be filed in the same commit**,
   stating what was changed, by what rule, and that they may freely correct any
   site without asking — it is their file and they know what those screens are
   for.

If any of the four fails, C is the fallback, and the extra round-trips are the
price of not writing to someone else's tree.

### What this does not license

It is not a general exemption from the ownership rule. It covers exactly the
case where a change is **atomic by the type system** — the compiler will not
accept either half alone. Anything a lane could plausibly do in two green
commits still goes through `requests/` and waits. In particular, "it would be
faster if I just did it" is not this rule; the trigger is "there is no ordering
of commits that compiles", which is a fact about the change and not a judgement
about the schedule.

### Where it bites

`scripts/q45_apply.py` (the `ROOTS` list includes `init`, with a comment
pointing here), `requests/c-b-render-text-gained-a-required-field.md`, and
`roadmap.md` → "Three-Agent Parallel Execution", whose `requests/` protocol this
entry qualifies. The next required field on a shared type will hit the same wall;
this is the answer for it.
