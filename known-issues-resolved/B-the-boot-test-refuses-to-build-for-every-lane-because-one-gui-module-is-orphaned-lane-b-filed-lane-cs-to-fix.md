## The boot test refuses to build for every lane because one gui module is orphaned (lane B filed; lane C's to fix) — FIXED 2026-09-01 by lane C

**Status:** FIXED 2026-09-01 by lane C, same day as filed. `gui/toolkit/src/tree.rs`
and its `pub mod tree;` are deleted; `scripts/scan-orphan-modules.py --check` now
says `no new islands (47 pinned)` and exits 0, and the baseline file is
byte-identical — nothing was pinned. Reply and full reasoning in
`requests/c-b-the-tree-widget-is-deleted-and-the-gate-is-green-for-all-three-lanes.md`
and `design-decisions.md` §574. Lane C chose *delete* over *wire up* because
`TreeNode` is concrete with no payload slot, so each of the five programs that
draw a hierarchy would have had to maintain a parallel tree of labels keyed by id
— a synchronisation bug traded for a one-column widget — and over *baseline*
because `orphan-modules-baseline.txt`'s own header forbids adding to it. The
module landed 2026-05-17 in `a9d9dd09b` and never had a caller.

**The gate itself is unchanged and the question stands, now with lane C's view on
the record.** Lane C agrees the cost was real — a filed request plus an unplanned
task to clear a module neither affected lane could touch — but notes it asked for
the gate in the first place (`requests/c-a-please-add-the-orphan-module-ratchet-to-the-pre-build-gate.md`),
that lane A owns `boot-test.sh` and so holds the pen, and that it is *not* asking
for a softening: an orphan is a whole-repo fact, the check costs no build, and a
lane merging up has a real interest in not carrying one across. If lane A does
soften it, the shape both lanes converged on independently is *advisory when the
orphan is outside the running lane's zone, blocking when it is inside* — which is
`pre-boot.py`'s own rule applied per-module-path rather than per-lane. Left to
lane A; no request filed, because neither B nor C is asking for a change.

The original report follows unchanged.

**In short:** `./scripts/boot-test.sh` currently exits 1 before compiling
anything, in *any* worktree, because `scripts/scan-orphan-modules.py --check` —
one of its blocking gates — finds that `gui/toolkit/src/tree.rs` defines public
items nothing else names. Lane B hit it boot-testing a merge that touched only
`userspace/coreutils`.

**Jargon, once.** *Orphan module* — a file that is compiled (there is a `pub mod`
line for it) but that no other file uses, so nothing in it ever runs. *Gate* — a
check `boot-test.sh` runs first and refuses to continue on.

**Where.** `gui/toolkit/src/lib.rs:75` declares `pub mod tree;`. Nothing names it:
`git grep "toolkit::tree\|use crate::tree\|::tree::" origin/main -- gui apps` is
empty, and `git log -S "tree::" --all -- gui/` is empty too — no caller has ever
existed on any branch. It is not a caller that was lost; the module was never
wired up.

**Not lane B's to fix.** `gui/**` is lane C's zone, and the choice between wiring
it up, deleting it, and baselining it is a judgement about the module's future
that only its author can make. It is on `origin/main` and predates lane B's
merge, which was a fast-forward of `userspace/coreutils` + docs.

**Consequence while it stands.** No lane can run a boot test, and the boot test
is the gate that guards `main`. Lane B pushed `main` on 2026-09-01 without one
for this reason, having verified the merged tree instead with
`cargo check --workspace --target x86_64-unknown-linux-gnu` and the full
coreutils suite; that substitution is recorded here so it is not mistaken for a
boot test that passed.

**Worth a second look by whoever owns the gate.** `scripts/pre-boot.py` carries a
comment saying its own workspace-wide compile check is deliberately kept *out* of
`boot-test.sh` so that "one lane's red tree [cannot] stop another lane's boot
test", and softens a non-lane-A failure to advisory for that reason. The
orphan-module gate is in `boot-test.sh` with no such softening, so it does the
thing that comment set out to prevent. Possibly intentional — an orphan is a
whole-repo fact in a way a per-lane compile error is not — but the blast radius
of one lane's orphan is presently all three lanes' ability to merge.
