## 1444. A diff's hunks are read in their files' languages, flat diffs included

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a diff of a Rust file now shows its code in Rust's colours:
the lines the new file has (kept and added) read together as one piece of
code, the old file's as another. The grammar's own rule for this finds
hunks only under a `diff --git`-style line; a plain `diff -u` of two files
has none, nor has `svn diff`, so a second rule of our own reads those. And
a file named with a timestamp after it, as `diff -u` and `diff -r` name
one, is known by its name alone.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **The published query, plus `injections.flat.scm` of our own for diffs with no `diff` line; a header's name cut at a tab** (chosen) | git's diffs, `diff -u`'s, `diff -r`'s and `svn diff`'s all coloured | a query of our own to keep in step with the grammar's -- its patterns mirror the published ones, and tests hold both shapes |
| The published query alone | nothing of our own | `diff -u` of two files -- the commonest diff outside git -- uncoloured; and a timestamped header names no language, so `diff -r`'s blocks lost their colours too |
| No injection: the diff's own colours only | a change's colour on every character | the code unreadable as code |

**Two readings of Neovim's directive `#offset!`.** It moves a range's ends
by rows and columns. A column past a line's end goes on into the next line,
the end of the line counting one column whether it is `\n` or `\r\n` -- so
the query's `0 1 0 1`, meant to take in each line's newline, takes in all of
a Windows file's `\r\n`; counted in bytes it would stop between the two, and
a `//` comment would run on into the next line. And where Neovim keeps a
capture whole when its offset would turn it inside out, it is dropped here:
a text too short to trim has nothing in it to colour.

**Revisit if** tree-sitter-diff gives a flat diff's hunks a block of their
own -- the flat query then goes -- or publishes a reading of either.
