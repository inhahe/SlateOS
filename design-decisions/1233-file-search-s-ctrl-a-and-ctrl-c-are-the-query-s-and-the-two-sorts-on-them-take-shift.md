## 1233. File search's Ctrl+A and Ctrl+C are the query's, and the two sorts on them take Shift

**Date:** 2026-10-04
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** the file search window types every key into its query, and
sorts its results with Ctrl and a letter -- among them Ctrl+C for "by
category" and Ctrl+A for "by path". When the query became a box with a
caret (selection, copy, cut and paste), those two letters collided with
select-all and copy, the two keys every text box answers. Now Ctrl+A
selects the query and Ctrl+C copies from it, as everywhere else, and the
two sorts keep their letters with Shift added: Ctrl+Shift+C sorts by
category, Ctrl+Shift+A by path. Home and End move the caret, so the first
and last result are Ctrl+Home and Ctrl+End. The list of keys (F1) says so.

### Context

`known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box.md`:
the query took typing at its end and Backspace from it, and nothing else.
`textline::apply_key` gives a box Ctrl+A, C, X and V; filesearch's keys
had Ctrl+N, S, M, E, C and A for its six sort columns ("on Ctrl so the
bare letters stay available as query text"), so two of the box's four
clipboard keys were already sorts.

### What was decided

- Ctrl+A and Ctrl+C go to the query: select all, copy.
- Category and path sorts move to Ctrl+Shift+C and Ctrl+Shift+A, checked
  ahead of the query (textline would otherwise take Ctrl+Shift+A as
  select-all, Shift being allowed in its chords).
- Home and End are the query's; Ctrl+Home and Ctrl+End select the first
  and last result.

### Alternatives

| Option | For | Against |
|---|---|---|
| Keep Ctrl+A and Ctrl+C as sorts; the query gets only Ctrl+X and V | no shortcut moves | a text box without select-all or copy, in a search tool whose query is worth copying |
| New letters for the two sorts (Ctrl+G "group", Ctrl+T "tree") | single chords | weak mnemonics, and two more letters to learn instead of one modifier |
| Ctrl+Shift with the same letters (chosen) | the letters people learned stay, Shift is the only change | a three-key chord for two of six sorts |

### Easy to reverse

The sort keys are two `match` arms at the top of
`FileSearchApp::handle_key` and two rows of `SHORTCUTS`;
`the_sorts_that_shared_the_clipboards_keys_take_shift` pins them.
