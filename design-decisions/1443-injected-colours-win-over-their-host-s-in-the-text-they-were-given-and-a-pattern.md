## 1443. Injected colours win over their host's in the text they were given, and a pattern's priority is read

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** when one language sits inside another -- a Rust file's lines
inside a diff, a code block inside Markdown, HTML inside a JavaScript
template -- both colour the same text, and something has to decide which
colour shows. Until now whichever started later won. That showed a diff's
added line inside a Rust comment in the "added" colour instead of the
comment's. Now a query's own word on importance (Neovim's `priority`)
decides first; then the inner language wins over the outer, but only on
the text the inner language was given to read; only then does the start
order decide, as before.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **Priority, then the inner language over the outer within the inner's own text, then start order** (chosen) | a diff's code coloured as code wherever it is, its `+` and `-` in their line's colour as the query asks; the holes in an injected language -- a template's `${...}`, a diff's markers, Markdown's `>` inside a quoted paragraph -- keep the outer language's colours | departs from tree-sitter's own highlighter, whose single stack the old rule copied event for event |
| Start order only -- tree-sitter's highlighter, the old rule | exactly tree-sitter's behaviour | an outer colour starting inside an inner one hides it: a diff line's colour over the second line of a comment; and `priority` unread, so a diff's markers are drawn as punctuation, and once the code is coloured the change shows only on plain names |
| Neovim's rule exactly: priority, then the inner language, nothing cut | the rule the diff's queries were written for | an inner node spanning a hole paints the hole: a comment the new file opens before a deleted line and closes after it paints the deleted line as a comment; an HTML attribute's string paints the JavaScript `${...}` inside it |

**What `priority` is.** Neovim's directive `(#set! priority 95)` -- or
`(#set! @capture priority 95)` for one capture: 100 where none is set;
higher shows over lower wherever both are, however their nodes nest; among
one node's captures, the higher beats a later pattern. Of the queries
vendored, only the diff's sets one: on the `+` and `-` markers, to put them
under their line's colour.

**Revisit if** a query turns up that needs an outer language's colour over
an inner one's inside the text the inner language was given.
