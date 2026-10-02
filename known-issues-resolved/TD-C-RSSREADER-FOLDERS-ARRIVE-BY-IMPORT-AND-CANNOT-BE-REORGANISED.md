## `TD-C-RSSREADER-FOLDERS-ARRIVE-BY-IMPORT-AND-CANNOT-BE-REORGANISED` -- **FIXED 2026-09-18** (lane C)

**In short:** `apps/rssreader` could not move a feed between folders, remove a
folder, or rename a feed. All three are now bound to keys. Chasing why they had
no callers found the actual cause, which was larger than the three operations:
**nothing in the app could select a feed**, and the help overlay was
advertising twenty-one shortcuts of which about four worked.

**The root cause.** `sidebar_selection` -- the field naming which feed or
folder is chosen -- had **no production writer**. It was `AllFeeds` from
construction to exit. So:

- the `Feed`, `Folder` and `Starred` arms of the article filter were
  unreachable: you could never narrow the list to one feed, which is what a
  sidebar is *for*;
- four highlight branches in the draw were dead;
- tabbing to the sidebar changed only which pane was outlined;
- and the three filed operations had no subject to act on, which is why nobody
  had called them.

`Folder::is_expanded`, `sidebar_visible` and `sort_order` had no writers
either -- so folders could never be collapsed, the sidebar could never be
hidden, and the window displayed "Sort: Date (newest first)", a label that
could not say anything else, above a `sort_by` that genuinely worked.

**The overlay was the thing that made this a lie rather than a gap.** Pressing
`?` listed twenty-one shortcuts. `R`, `Shift+R`, `Shift+J`, `Shift+K`, `B`,
`O`, `F`, `D`, `V`, `Space`, `/`, `A`, `Ctrl+R` and `Ctrl+N` were named and
bound to nothing, and three more named operations that existed **nowhere in
the crate**: refresh (impossible -- this reader has no transport), open in
browser (there is no browser), and quit (the framework gives an app no way to
close its own window).

**What was done.** All of the above are bound, over machinery that already
existed and was already obeyed. The four impossible rows were removed from the
overlay and four real ones it had never mentioned were added. Removals ask
first and only `Y` confirms; removing a folder keeps its feeds. `A` adds a
feed by address, so the app now has a whole subscription workflow -- add,
rename, file into a folder, export OPML -- none of which needs the fetching it
cannot do. 216 tests, up from 184.

**The guard that keeps it fixed** is `every_advertised_shortcut_does_something`:
it walks `ALL_KEY_ACTIONS` and asserts each row's key is answered. A new row
must be given a binding, and a binding that disappears fails the test. **It
caught one defect on its first run** -- `Space` was a silent no-op unless a
folder was selected -- which is the argument for the pattern: the overlay and
the handler are two lists that must agree, and nothing but a test makes them.

**Worth copying.** Any app with a shortcut table should have this test. The
failure mode is invisible by construction: a key that does nothing looks
exactly like a key you pressed wrong, so users blame themselves and the bug is
never reported.
