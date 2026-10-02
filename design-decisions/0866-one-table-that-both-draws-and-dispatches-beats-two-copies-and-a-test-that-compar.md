## 866. One table that both draws and dispatches beats two copies and a test that compares them

**Date:** 2026-09-21 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** when a program shows you a list of its keyboard shortcuts, that
list and the code that answers the keys are usually two separate things, and
they drift apart -- the list ends up promising keys that do nothing. The usual
answer here has been to write a test that compares them. `apps/videoplayer`
does something better: it keeps *one* table that is both the thing drawn on
screen and the thing the key handler looks the key up in, so there is nothing
to compare and nothing to drift.

**What the two shapes look like.**

*Two copies, guarded* -- the common shape in this tree, and what
design-decisions 863 describes:

```rust
const SHORTCUTS: &[(&str, &str)] = &[("Ctrl+S", "Save")];   // drawn
match key { Key::S if ctrl => self.save(), .. }             // dispatched
#[test] fn every_advertised_key_does_something() { .. }     // compared
```

*One table* -- `apps/videoplayer`:

```rust
pub struct Shortcut {
    pub keys: &'static str,      // as the help panel prints it
    pub action: &'static str,    // as the help panel prints it
    pub press: Press,            // the keystroke that runs it
    pub command: Command,        // what running it does
}
Shortcuts::list().iter().find(|sc| sc.press.matches(event))
```

The panel draws `keys` and `action`; the handler finds the row by `press` and
runs `command`. Adding a row to the panel *is* adding the binding.

**Why this is the better one, stated as a property rather than a preference.**
The guarded shape detects drift; the single table makes drift unrepresentable.
That difference is not academic here. `apps/paint`'s list was unguarded and 8
of its 33 rows were dead -- two for a Save and an Open no code path reached,
six for keys whose character never arrived. `apps/rssreader` shipped 21 rows of
which about four worked. `apps/towers` advertised no key for `N` while `N`
started a new game. In the single-table shape none of those is expressible: a
row with no working key is a row with no `command`, which does not compile.

`apps/videoplayer` also shows what the shape does *for* you. Its `list()` doc
comment records that `Ctrl+O` and `Ctrl+S` were removed because the tree has
neither a file chooser nor a framebuffer read-back -- the identical defect
`apps/paint` shipped, noticed and fixed here as a matter of course, because
deleting the row and deleting the binding are the same edit.

**So why is this not simply mandated everywhere?** Because the cost is not
uniform. The single table needs a `Command` enum and a table-driven dispatcher.
For a program that already has one -- a media player, an editor, anything with
an undo stack or a command palette -- that is free or nearly so. For a small
game whose handler is a dozen-arm `match` on `Key`, introducing a command enum
to hold nine rows is a larger change than the problem, and one with its own
risk: a mechanical rewrite of a working handler to satisfy a pattern.

**The rule adopted.** Prefer the single table when the app already has a
command type, or when its key handling is being restructured anyway. Otherwise
use `SHORTCUTS` plus the two guards from 863, which is cheap and catches the
same defects one step later. **Do not rewrite a working handler solely to adopt
the table** -- and when the guarded shape is chosen, say in the list's doc
comment that it is two copies, so the next reader knows the test is load-
bearing rather than decorative.

**What this does not change.** 863 stands: `F1` always, `?` where free. This is
about where the list *lives*, not which key raises it.

**A note on how it was found,** because it is the third time today this shape
has come up. `apps/videoplayer` appears in the survey of apps that "bind keys
and carry no list" -- 31 keys, no list. It has the best key documentation in
the suite. The survey looks for a `const` of `(&str, &str)`, and this table is
a `&'static [Shortcut]`, so it was invisible to it. The survey is right to
report candidates rather than verdicts, and this is the third app off that
queue that turned out to need nothing -- after `apps/sokoban`, which prints its
keys in a footer, and `apps/minesweeper`, which was counted unguarded because
its guard has a different name.
