## TD-C-SIX-TOOLKIT-WIDGETS-ARE-WRITTEN-TESTED-AND-USED-BY-NOTHING

**Date:** 2026-09-14. **Lane:** C.

**In short:** the GUI toolkit contains six finished, tested components that no
program anywhere in the tree refers to -- about thirteen thousand lines and two
hundred and sixty tests of working code that no user can reach. Some of them do
a job an application is currently doing worse by hand, which is the part that
costs something: the file explorer classifies files with its own hardcoded list
of extensions while `guitk::filetypes` sits unused, and its own module doc says
in as many words that this is what must not happen.

**How this was found.** Not by a sweep -- by wiring `guitk::pathbar` into the
file explorer on 2026-09-14 and noticing it had been carrying
`#![allow(dead_code)]` and had no users. Asking the same question of every
`pub mod` in `gui/toolkit/src/lib.rs` gave the list below.

| module | lines | tests | files mentioning it |
|---|---|---|---|
| `menubar` | 3 490 | 61 | 0 |
| `svg` | 3 391 | 47 | 0 |
| `filetypes` | 2 201 | 41 | 0 |
| `disabled` | 1 754 | 41 | 0 |
| `context_ext` | 1 602 | 53 | 0 |
| `signal` | 853 | 20 | 0 |

(Counted as: no file outside `gui/toolkit/src/<module>.rs` names `<module>::`
or `guitk::<module>`. `pathbar` was a seventh until this morning; `fontdb` and
`row_strip` have a mention each and are not counted here.)

**`menubar` is the one to be embarrassed about.** It was edited *this session*
-- the viewport sweep threaded a `viewport` argument through
`MenuBar::handle_mouse_event` and `handle_key_event` and updated its tests --
without anyone noticing that no program opens a menu bar. Work was done to a
component and its tests, carefully, while the component reached nothing. That
is the same shape as `apps/automator`'s mutation table: it proved the worker
and never knocked on the door.

**`filetypes` is the one that is actively costing something.** Its module doc
opens with

> Every GUI component that needs to display, open, or classify a file should go
> through this module rather than hard-coding extension lists.

and `apps/explorer/src/main.rs` has a `FileType` enum with its own
`from_extension` match over about fifty extensions, plus two further extension
matches in `apps/explorer/src/columns.rs`. So the rule the module states is
broken three times in the one application that most obviously needs it, and the
registry with the magic-byte signatures and MIME types goes unread. A file the
explorer calls "WEBP File" is one `filetypes` knows the category of.

**What to do with each, which is not the same answer.** A component with no
consumer is either a feature the user cannot reach or code to delete, and
deciding which needs the question "who would use this?" asked per module:

* `filetypes` -- **wire it.** The consumer exists and is doing the job worse.
* `disabled` -- probably wire it. It carries a *reason* a control is disabled,
  which is better than the bare `bool` the explorer's toolbar was given on
  2026-09-14; that bool was written without checking here first, which is the
  habit this entry is really about.
* `menubar` -- **wired 2026-09-14**, into `apps/editor`. See the correction
  below: the sentence this bullet used to carry was wrong.
* `context_ext` -- wants an application with a menu bar. The shell has its own
  menus; whether a second implementation should exist at all is a design
  question, not a wiring one.
* `svg` -- a renderer with no caller. `apps/imageviewer` and the icon paths are
  the candidates; 3 391 lines is worth an hour's look before either wiring or
  deleting.
* `signal` -- **deleted 2026-09-14**, design-decisions 851. It was an observer
  mechanism, and this tree does not observe: 138 crates under `apps/` and
  `gui/` take `handle_event(&Event) -> Response` and none connected a signal.
  The distinction that decided it: an unreachable *widget* becomes reachable
  when an application draws it -- which is what happened to the other five on
  this list -- but an unreachable *architecture* becomes reachable only by
  rewriting the architecture.

**Corrected within the hour: there IS a gate, and it is better than this
entry first said.** `scripts/scan-orphan-modules.py` asks exactly this
question, covers `gui/`, and reports `menubar`, `svg`, `context_ext` and
`signal` by name. `pathbar` was in its baseline until this morning, and
`--check` printed *"reached now, drop from the baseline"* the moment the file
explorer used it. The first version of this paragraph said no gate looked at a
library crate's public surface. That was written without running the gate, and
it is the same failure the rest of this file is about.

**What is actually wrong is narrower and worse.** The scan cleared
`gui/toolkit/src/filetypes.rs` while *nothing at all* used it -- verified by
checking out the pre-change explorer and running the scan again, which still
did not report it. Lane A pruned it from the baseline on 2026-09-11 as
"reached by lane C's own later work", consistently with what the gate said;
the gate was wrong, not the prune.

**Why it was wrong.** A mention is any identifier token equal to one of the
module's public item names, anywhere in the tree. The candidate loop skips
`main.rs`, so a type defined in an application's `main.rs` is never registered
as *another owner* of that name -- but its every appearance still counts as a
mention of the library module. `apps/fileassoc/src/main.rs` declares its own
`FileCategory` and its own `FileType`, and those two names alone were enough to
clear a 2 201-line registry that no file in the tree referred to.

The docstring already anticipates this class -- *"names shared with another
module ... are dropped from the evidence entirely"* -- and the rule does not
reach a name whose other owner is a `main.rs`. The fix is to collect
shared-name owners from every Rust file rather than from candidates only; a
file that is not a candidate module can still own a name.

**What it cost.** Three hard-coded extension lists in `apps/explorer` and a
fourth model in `apps/fileassoc`, all beside an unread registry with MIME types
and magic-byte signatures, for three days after the gate said the registry was
fine. Filed as
`TD-C-THE-ORPHAN-SCAN-CLEARS-A-MODULE-ON-A-NAME-AN-APP-HAPPENS-TO-SHARE`.

**Correction, 2026-09-14: `menubar` was never a design question, and saying it
was is what kept it unwired for a day longer.** The plan above deferred it as
*"the shell has its own menus; whether a second implementation should exist at
all is a design question"*. That reasoning only holds if the shell is the
consumer -- and the consumer is `apps/editor`, a text editor with five thousand
lines, eleven commands, and no menus of any kind. There is no second
implementation and no conflict: the shell's menus are the compositor's, an
application's menu bar is the application's, and the two never meet. The
question I should have asked was the one this entry's own heading asks --
*who would use this?* -- and I answered it for the shell without asking it of
the applications.

That is the second time in this entry I deferred something on reasoning I had
not checked; the first is the paragraph above about the gate. The pattern is the
same both times: a confident sentence about the state of the tree, written
without reading the tree.

**What `menubar` wiring actually turned up**, which is the part that makes it
worth more than one feature:

* `MenuBar::set_items` **closes any open dropdown**, and its doc said only
  *"Replace the entire menu structure"*. The editor rebuilds its rows so a
  greyed-out Undo is greyed for a live reason, and doing that on every event
  shut the menu on the user's first arrow key. Caught by a test, not by
  reading. Its doc now says what it does.
* The editor's `TAB_BAR_HEIGHT` was doing duty as both *the strip's height* and
  *the y where the text starts* -- equal only while nothing sat above the
  strip. `visible_lines` already carried a comment about the last time two
  copies of that number disagreed (a hardcoded 64 for a 32-pixel strip, which
  under-reported the viewport by two lines). Five tests were hardcoding `y =
  10.0` to mean "inside the tab strip" and broke the moment it moved, which is
  the same defect in the tests.

**Addition, 2026-09-24: `treeview` and `dirtree` join the list on the day they
were written, and the reason is the six-lane split rather than an oversight.**
They are the toolkit's treeview and `design.txt`'s tristate checkbox treeview
with its populate-from-a-directory function. Five applications hand-roll a tree
and are the obvious first consumers -- `archivemanager`, `jsonviewer`,
`devicemanager`, `dbviewer`, `diskanalyzer` -- but since 2026-09-22 `apps/**` is
lane E's, so lane C can build the widget and cannot wire it, which is exactly
how the modules above came to exist without users. Handed over, with a mapping
per application, in
`requests/c-e-the-toolkit-has-a-treeview-now-and-five-apps-draw-their-own.md`.
Nothing in lane C's own tree draws a tree today; the start menu's
"Applications tree" (roadmap-detailed §3.4) would, once C-Q20 settles which
list of installed programs is the real one.
