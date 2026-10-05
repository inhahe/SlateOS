## 1442. Go's highlight query is read with its general patterns first

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** Go's colouring rules say "a function's name is a function"
and, further down, "every name is a variable". Tree-sitter's highlighter --
and the editor's (§1438) -- lets the later rule win where two colour the
same piece of code, so read as published every Go function's name came out
a plain variable and every method a property. The rules were written when
the first rule won, and the grammar ships no tests of its colours that would
have shown the change. The editor reads the same rules with the general ones
moved to the top, so the specific ones win, as their author meant.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **The published patterns, the general block (`type_identifier`, `field_identifier`, `identifier`) moved to the top** (chosen) | functions and methods coloured as functions; every pattern upstream's, no more and no fewer | a file of our own beside the published one (`highlights.general-first.scm`), which a test holds to the published one reordered, so an update shows at once |
| The published order | exactly as published | function and method names coloured as variables and properties |
| Neovim's Go query instead | written for the last-wins rule | another project's query, with predicates this runtime does not evaluate (`#lua-match?`), and captures named for another editor |

**How it was found, and that it is the only one.** A test of the editor's
own colouring of Go failed on a function's name. A scan of every vendored
query for a bare `(node) @capture` coming after a more specific pattern on
the same node found two: Go's, and CSS's `--custom` properties -- whose
`@variable` pattern CSS's own highlight tests show is meant to lose (they
expect `--color1` to be a property). TypeScript's order was already the
subject of §1441.

**Revisit if** tree-sitter-go reorders its query or ships highlight tests;
then the published query is read as it is, and this file goes.
