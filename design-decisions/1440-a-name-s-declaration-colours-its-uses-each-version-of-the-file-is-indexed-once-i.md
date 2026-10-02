## 1440. A name's declaration colours its uses: each version of the file is indexed once, in the background

**Date:** 2026-09-28 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** JavaScript's colouring rules -- and TypeScript's, which build
on them -- colour a function's parameters wherever they are used, not only
where they are declared, and stop colouring `module` or `console` as
Node's own where the code declares a variable of that name. They do it
through a third query besides colours and injections, `locals.scm`, which
says where the scopes are, which names each declares, and which names may
use one; tree-sitter's own highlighter reads it, and the code editor's now
does too, the same way. A use can be thousands of lines below its
declaration, where the editor -- which colours a screen at a time --
never looks, so each version of the file is indexed once, in the
background, a few milliseconds per frame. Until the index for the edited
file is ready, the previous one is used, moved along with the edits.

**What it costs** (1.1 MB of real JavaScript -- npm's own modules, end to
end -- in a release build, with a workspace test running alongside): the
pass takes 0.3 s, all of it tree-sitter's query cursor, which takes as long
with this crate's bookkeeping taken out; parsing the file from scratch takes
0.7 s. After a keystroke the reparse and the pass together take 0.33 s of
background work, spread a few milliseconds over each frame; a file of
typical size, 20 KB, a few milliseconds in all. Drawing a screen takes 4.5
ms with the index and 4.4 ms without; moving the index with a keystroke,
0.17 ms. A declaration's colour is found once, the first time a use of it is
drawn, by a query started just above the declaration -- as many levels up
as the query's patterns nest (`pattern_depth`) -- rather than at the root,
from which it steps past everything before the declaration: the first
screen at the end of the large file draws in 6.9 ms that way, against 17.8
ms from the root and 3.7 ms for each draw after, once the colours are
known.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **Index each tree once, in `work()`'s slices; move it with the edits until the next is done** (chosen) | exact -- as tree-sitter's highlighter reads the query -- once the pass is done; never on the drawing path; nothing flickers while it runs | a pass for every edit: background work in proportion to the file (0.3 s for 1.1 MB); until the pass after an edit is done, a use the edit touched is shown as a plain name |
| Ignore `locals.scm` (Neovim's choice) | costs nothing | parameters coloured only where declared; a declared `module` still coloured as Node's; JavaScript's own highlight test `variables.js` fails |
| Run the locals query from the top of the file on every draw | exact, and no index to keep | every frame pays for a pass over everything above the screen: 0.3 s on the large file |
| Run it over the screen alone (Helix, until its rewrite) | cheap | a parameter's uses change colour as its declaration scrolls off the top |
| An incremental index, redoing only what follows an edit | less background work | what follows an edit must be redone anyway -- a declaration changes how everything after it resolves -- so it saves half a pass on average, for the machinery of saving the pass's state as it goes |

**Where it follows tree-sitter's highlighter to the letter**, since the
queries are written and tested against it: a scope is closed only by a node
starting *past* its end, so two blocks back to back (`{ ... }{ ... }`) nest;
a use is of the last declaration of its name, before it, in the innermost
scope that has one, looking outward only through scopes that inherit
(`local.scope-inherits`), and not of a declaration whose value it is inside
(`local.definition-value`); of a local's captures, the first paints even if
its pattern is marked `(#is-not? local)`, and the later ones so marked do
not; a use of a declaration that paints nothing is no local. Captures whose
names have no colour here take no part, as §1438 has it for every node.

**How it is known to be right.** JavaScript's highlight tests run here:
55 assertions, 26 of them `variables.js`'s, seven of which -- a
parameter's uses, `module` declared in a block -- fail without this. Tests
of this crate's own pin the rest: the pass in slices finds what one pass
does, with scopes that inherit and scopes that do not; a colour found near
its declaration is the one found from the root, for every declaration of a
file of them; the rules above, one by one, with queries that use what
JavaScript's does not; an edit moving the index, and forgetting what it
touched -- a name an edit runs into the next is not its declaration's
meanwhile; an injected stretch indexed as it is parsed, and carried on by
`work` when it does not fit a draw. Twenty-two mutations of it each fail
one of these; one more is no change at all (a use looked up on a node that
also declares a name, where the declaration always wins).

**Revisit if** the pass after each keystroke shows up as a problem on large
files (keeping the pass's state at points along the file would let it
resume from before the edit), or tree-sitter's query cursor gains a way to
skip to a position rather than step through every sibling before it, which
would cut both the pass and the drawing of a screen on large files.
