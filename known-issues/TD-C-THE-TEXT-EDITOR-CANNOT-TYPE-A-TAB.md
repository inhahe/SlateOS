## `TD-C-THE-TEXT-EDITOR-CANNOT-TYPE-A-TAB` -- **PARTLY FIXED 2026-09-18** (lane C)

**In short:** `apps/editor` indents with spaces and cannot be told otherwise.
`Document::use_spaces` is `true` at construction and **has no production
writer**, so pressing Tab always inserts spaces, and the status bar's
indentation readout can only ever say "Spaces: N" and never "Tab width: N".
**A Makefile cannot be edited correctly in it**, because `make` requires a
literal tab, and the same goes for Go.

**Verified.**

| | |
|---|---|
| the field | `pub use_spaces: bool`, set `true` at both constructors |
| writers in production | **none.** The three assignments (`d.use_spaces = true`, `doc.use_spaces = true`, `doc.use_spaces = false`) are all inside the test module -- `rustlex.live_code` blanks all three |
| what obeys it | `ch == '\t' && doc.use_spaces` converts a typed tab to spaces, and `let indent = if doc.use_spaces` picks the indent string |
| what displays it | the status bar, `format!("Spaces: {}", doc.tab_width)` versus `format!("Tab width: {}", doc.tab_width)` |

**The comment is the part worth keeping.** Beside the status-bar code stands:

> Indentation mode. The document already knows both halves -- `use_spaces` and
> `tab_width` are set when a file is read -- and `roadmap-detailed.md` §4.4
> asks for it in this bar; it was simply never drawn.

**Nothing sets `use_spaces` when a file is read.** There is no indent detection
anywhere in the crate -- no scan for leading tabs, no heuristic, nothing. The
comment describes a mechanism that does not exist, and it was written by
somebody *adding* the readout, who reasonably assumed the value behind it was
real because it had a name and a type. **A comment asserting a mechanism is
worse than silence**: it answers the next reader's question wrongly, and it
answered mine -- I nearly stopped looking when I read it.

**The tests are why the `false` branch is well-built and unreachable.** They
set `use_spaces` both ways and assert both behaviours, so the tab-indent path
is covered, correct and impossible to reach from the program. That is the same
shape as `apps/slides`'s themes and `apps/explorer`'s view modes: **complete
machinery, tested, with no way in.**

**Detection landed (2026-09-18), which is the half that matters.**
`Document::from_file` now decides indentation from the file, beside the
line-ending detection it mirrors -- both are properties of the document rather
than preferences of the program. The first indented line decides: a leading tab
means tabs, a leading space means spaces, an unindented file defaults to
spaces. A file that mixes them is already inconsistent and no answer serves it;
following the first is what an editor can defend. Opening a Makefile and typing
no longer corrupts it. 210 tests, up from 207.

**The toggle landed too (2026-09-18), so this is now fully fixed.** `Ctrl+T`
switches the active document, the status bar names the key beside the value it
already drew, and the menu row reads "Tabs or Spaces".

**Adding one command touched nine places**, and the crate caught five of them
for me: the compiler demanded a menu label, a shortcut string, an enabled
predicate, a dispatch arm and an arm in the guard test
`every_shortcut_a_menu_advertises_is_really_bound`, whose inner match is
exhaustive *on purpose* -- its comment says a new command should stop the file
compiling "until someone says what pressing its advertised key should do". Two
more were manual lists that do not fail loudly: `Command::ALL`, which decides
whether the guard test arm ever runs, and a `letter_key` name-to-key mapping
that panicked with "no key is named T".

**`Command::ALL` is the interesting one.** The exhaustive match forces the arm
to be *written*; `ALL` decides whether it is *run*. A variant left out of that
array compiles, with a test arm that never executes -- passing by accident, in
its purest form. The two have to be changed together and nothing makes that
true except noticing.

~~**Still missing: the toggle**~~, so a *new* file cannot be told to use tabs --
only an existing tab-indented one is honoured. The status bar already draws the
value, so it needs a key and a name beside it, not new machinery.

**What the repair wants.** Two things, and the second is the one that matters:
a key to toggle it (the status bar already draws the value, so it only needs
naming), and **detection on read** -- if any line begins with a tab, the file
uses tabs. Without the second, opening a Makefile and typing still corrupts
it, which is the case the whole finding is about.
