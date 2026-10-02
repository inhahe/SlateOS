## 574. `guitk::tree` is deleted rather than wired up or baselined: a widget no app can adopt is not an asset

**Date:** 2026-09-01
**Decided by:** Claude (autonomous)

**Lane:** C

**In short:** The toolkit had a TreeView widget — a 1,098-line module for
showing a folder-style expandable list — that no program in the repository has
ever used, at any point in the history. An automatic check that refuses to build
when a module has no callers went red on it, and because that check runs before
the compile in `boot-test.sh`, **all three lanes lost the ability to boot-test
or merge** until it was resolved. The three ways out were: find something to use
it, delete it, or add it to the ledger of modules that are allowed to have no
callers. It is deleted. The five programs in the tree that display a tree each
have their own, and none of them could adopt this one without keeping a second
copy of its data in step with the first.

Filed as `requests/b-c-gui-toolkit-tree-module-is-orphaned-and-blocks-every-lanes-boot-test.md`
by lane B, who correctly declined to fix it — `gui/**` is lane C's zone and the
choice is about the module's future, not about the gate.

### Why not wire it up

Because there is nothing that can use it as written, and making something use it
would mean making an app *worse*.

`TreeNode` is a concrete struct — `id`, `label`, `icon`, `expanded`, `selected`,
and an owned `Vec<TreeNode>` of children. There is no payload slot and no type
parameter. Every program in the tree that draws a hierarchy has a node that
carries its own domain data and draws its own columns:

| Program | Its node | What its rows show |
|---|---|---|
| `apps/devicemanager` | `TreeNode` over `DeviceInfo` | device class, status, driver |
| `apps/archivemanager` | `TreeNode` over archive paths | size, packed size, date |
| `apps/dbviewer` | `TreeNode` + `TreeNodeKind` | schema/table/column, row counts |
| `apps/jsonviewer` | `TreeViewNode` | key, value, type badge |
| `apps/systemrestore` | its own `render_tree_view` | restore point, date, kind |

Adopting `guitk::tree` in any of them means building a **parallel tree of
labels** keyed by id, and then keeping it in step with the real one on every
expand, collapse, filter and refresh. That is the two-structures-that-drift
failure this project keeps writing entries about (§524's triple copy of the
double-click range is the same shape), traded for a widget that draws one text
column where the app draws four. The app gets worse and gains a synchronisation
bug. "Wire it up" is only the right answer when a caller *wants* the module.

### Why not baseline it

`scripts/orphan-modules-baseline.txt` says so in its own header: the list "is a
debt ledger, not an allow-list, and the only edit it should ever receive is a
deletion… A new module lands wired up or it does not land." A `--pin` that adds
a line is named there as the exact failure the gate exists to prevent. The
baseline's honest purpose is the 47 modules that predate the gate; putting a
48th in would convert a shrinking ledger into a place to file new orphans, which
is how a ratchet stops ratcheting.

The baseline's stated use — "deliberately ahead of its caller, e.g. a widget
landed before the app that will host it" — does not apply. This module was
committed on 2026-05-17 in `a9d9dd09b` ("add TreeView and TabView widgets") and
`git log -S "tree::" --all -- gui/` finds no caller on any branch at any time.
Its sibling `TabView` from the same commit did find one. After three and a half
months and five programs that draw trees choosing not to use it, "the caller is coming" is not a
forecast, it is a description of a decision nobody made.

### What is actually lost, and why it is small

1,098 lines including its own tests, all of them tests of code no user reaches.
Two properties make the loss cheap:

- **The git history keeps it.** If a caller ever appears, `git show
  a9d9dd09b:gui/toolkit/src/tree.rs` restores it — and whoever restores it will
  want to change it anyway, because it is written against the pre-`Frame` API
  (`render(x, y, w, h) -> Vec<RenderCommand>`, no hit boxes) and hard-codes
  seven Catppuccin colour constants instead of reading `guitk::theme`. A
  resurrection would be a rewrite with the old file as a reference, which is
  precisely what history is for.
- **Keeping it is not neutral.** An unused module that looks finished is a trap:
  the next person who needs a tree finds it, adopts it, and discovers only then
  that it cannot record a click target and does not follow the theme. Dead code
  that reads as available is worse than absent code, because absent code sends
  you to write what you need.

### The gate's blast radius — noted, not changed

Lane B's request points out that `scripts/pre-boot.py` deliberately keeps its
own workspace compile check *out* of `boot-test.sh` so that "one lane's red tree
[cannot] stop another lane's boot test", while the orphan-module gate is in
`boot-test.sh` with no such per-lane softening — so one lane's orphan blocked
all three. Lane C asked for that gate (`requests/c-a-please-add-the-orphan-module-ratchet-to-the-pre-build-gate.md`)
and lane A owns `boot-test.sh`, so the softening is not lane C's to make. It is
also not obviously wanted: an orphan is a whole-repo fact and the gate is
cheap (it does not build), so a lane that is merging *up* has a legitimate
interest in not carrying one across. The counter-argument — that a lane which
cannot fix the orphan, because it is in another lane's zone, still cannot boot —
is exactly what happened here, and cost lane B a filed request and lane C an
unplanned task. Left to lane A with the observation on the record; the reply
request carries it.

**Where it lives:** deleted — `gui/toolkit/src/tree.rs`, and `pub mod tree;`
from `gui/toolkit/src/lib.rs`.
