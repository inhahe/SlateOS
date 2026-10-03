### [C] The linker-script grammar does not know all of ld's language -- 2026-09-29

**Status:** OPEN -- a limitation, not a failure: the file is coloured, and
the parts the grammar does not know are coloured by error recovery.

**In short:** the code editor colours linker scripts (`.ld`) with
tree-sitter-linkerscript 1.0.0, the grammar every editor uses for them. It
has no rule for several things real scripts write -- the kernel's own
among them -- so those parse as errors: a program header's `FLAGS(...)`,
an output section's `ALIGN(...)` (or any attribute) after its colon, a
second `:phdr` after a section, and the commands `INCLUDE`,
`OUTPUT_FORMAT` and `OUTPUT_ARCH` (and, by the look of the grammar,
`SEARCH_DIR`, `INPUT`, `GROUP`, `TARGET` and the like too). What is around
them is read correctly.

**Where:** `gui/syntax/grammars/linkerscript/parser.c` (generated, as
published). `gui/syntax/src/grammars/linkerscript.rs`'s test pins exactly
the gaps the tree's scripts and one example hit, so a grammar that learns
one shows at once.

**The proper fix:** add the missing rules to the grammar's `grammar.js`
(tree-sitter-grammars/tree-sitter-linkerscript), regenerate `parser.c`
with the tree-sitter CLI, and vendor that as a fork of our own, named so
beside the published one -- then offer the rules upstream, and go back to
the published grammar when it has them. The converter reads a newer
generator's output as well as this older one's (`gui/tsgrammar`).
