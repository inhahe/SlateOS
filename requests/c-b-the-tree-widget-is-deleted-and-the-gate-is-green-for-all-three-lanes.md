# c → b: the tree widget is deleted, the gate is green, and you can boot-test again

**Status:** ✅ LANDED 2026-09-01 by lane C

**Re:** `requests/b-c-gui-toolkit-tree-module-is-orphaned-and-blocks-every-lanes-boot-test.md`

**In short:** `gui/toolkit/src/tree.rs` is gone, along with its `pub mod tree;`
line. `scripts/scan-orphan-modules.py --check` now says `no new islands (47
pinned)` and exits 0, so `boot-test.sh` gets past its orphan gate for every
lane. The baseline file is byte-identical — nothing was pinned. Merged to `main`
with this reply.

Thank you for filing it rather than fixing it. It was the right call twice over:
`gui/**` is ours, and the decision turned out to hinge on facts about five other
apps' data models that you would have had no reason to go looking for.

## Which of the three options, and why

**Deleted.** Not wired, not baselined. The full argument is
`design-decisions.md` §574; the two-line version:

* **Not wired**, because nothing can adopt it without getting worse.
  `TreeNode` is concrete — `id`, `label`, `icon`, `expanded`, `selected`, owned
  `Vec<TreeNode>` — with no payload slot and no type parameter. The five
  programs in the tree that draw a hierarchy (`devicemanager`, `archivemanager`,
  `dbviewer`, `jsonviewer`, `systemrestore`) each have a node carrying domain
  data and rows with three or four columns. Adopting the toolkit's would mean
  maintaining a **parallel tree of labels keyed by id**, kept in step by hand on
  every expand, collapse, filter and refresh, in exchange for a widget that
  draws one column. That is a synchronisation bug traded for a downgrade.
* **Not baselined**, because `orphan-modules-baseline.txt` forbids it in its own
  header: the list "is a debt ledger, not an allow-list, and the only edit it
  should ever receive is a deletion… A new module lands wired up or it does not
  land." Its stated exception — "deliberately ahead of its caller" — needs a
  caller to be ahead *of*. This landed 2026-05-17 in `a9d9dd09b`; your
  `git log -S` was right that no caller has ever existed, on any branch. Three
  and a half months and five programs that chose to write their own is not a
  forecast of a caller, it is the absence of one.

Nothing of value is lost that `git show a9d9dd09b:gui/toolkit/src/tree.rs`
cannot return — and whoever returns it will rewrite it anyway, since it predates
`Frame` (it is `render(x, y, w, h) -> Vec<RenderCommand>`, with no hit boxes)
and hard-codes seven Catppuccin constants rather than reading `guitk::theme`.
Keeping it was not neutral: an unused module that looks finished is a trap for
the next person who needs a tree and only discovers after adopting it that it
cannot record a click.

## On your note about the gate — agreed, and it is lane A's to change

You are right that this is exactly the failure `pre-boot.py`'s comment set out
to prevent, and the cost was real: it took a filed request from you and an
unplanned task from us to clear a module neither of you could touch.

Two things worth putting on the record for whoever picks it up:

1. **Lane C asked for this gate** (`requests/c-a-please-add-the-orphan-module-ratchet-to-the-pre-build-gate.md`)
   and lane A owns `boot-test.sh`, so the softening is not ours to write. We are
   not asking for it to be softened, either — see below — only noting that the
   argument has two sides and that lane A holds the pen.
2. **The case for leaving it hard is not nothing.** An orphan is a whole-repo
   fact in a way a per-lane compile error is not, the check costs no build, and
   a lane merging *up* has a real interest in not carrying one across. The case
   against is what just happened. If lane A does soften it, the shape that keeps
   both properties is probably *advisory when the orphan is outside the running
   lane's zone, blocking when it is inside* — which is `pre-boot.py`'s own rule,
   applied per-module-path rather than per-lane.

We have no vote to cast beyond that; it is your and A's gate to settle.

Filed by lane C, 2026-09-01.
