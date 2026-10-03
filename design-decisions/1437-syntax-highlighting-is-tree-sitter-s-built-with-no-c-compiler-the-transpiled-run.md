## 1437. Syntax highlighting is tree-sitter's, built with no C compiler: the transpiled runtime, and each grammar's parser.c converted to Rust at build time

**Date:** 2026-09-28 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The code editor colours code the way most current editors do --
with tree-sitter, which parses the file into a tree and re-parses only what an
edit touched, and whose grammars already exist for nearly every language. Its
parts are written in C, and this project builds with no C compiler. So the
parser engine is the one already translated to Rust (a published crate), and
each language's grammar -- itself a C file that tree-sitter's generator
writes -- is translated to Rust when the editor is built, by a converter
written for that shape of file. Each grammar's own test examples run against
the translation, and pass, which is what says it parses as the original does.

**Why tree-sitter at all.** `roadmap-detailed.md` asks for "syntax
highlighting via tree-sitter integration". A regular-expression highlighter
(TextMate/Sublime grammars, `syntect`) colours by line and does not know a
file's structure; tree-sitter gives the tree too, which folding, outlines,
selection by syntax and indentation will want.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **Transpiled runtime + grammars converted at build time** (chosen) | pure Rust: builds for the host tests, the Linux lint and SlateOS alike; each grammar's `parser.c` stays verbatim (update = replace the file; diff against upstream any time); tables go in as bytes, so a 6 MB grammar compiles in seconds | a converter to maintain (`gui/tsgrammar`, ~1,400 lines with tests); each grammar's external scanner (`scanner.c`, hand-written C) is ported by hand; the runtime crate trails upstream (0.25.2 against 0.27) |
| The published `tree-sitter` crate and grammar crates, compiled as C | upstream exactly | needs a C compiler in every build: none on this machine's gates, none for `x86_64-slateos` |
| Grammars run as WebAssembly (tree-sitter's wasm support) | no conversion | a wasm runtime (wasmtime) in every program that highlights, far larger than the grammars |
| Our own parser framework, or regex highlighting | no third-party runtime | not tree-sitter: every grammar written again, and no tree |

**How it is known to be right.** Every grammar ships its authors' test corpus
(`tree-sitter test`'s examples: an input and the tree it must parse to); the
corpus runs here against the converted grammar with `tree-sitter test`'s own
comparison (`gui/syntax/src/corpus.rs`), and all of them pass: JSON's 6
examples, Rust's 151 and Python's 117. A converter bug or a scanner mistake fails them: mutating the
ported scanners fails their corpora. The `TSLanguage` mirror's layout is pinned
field by field, and the runtime is asked, through its own API, for the values
at the far end of the struct (a grammar's name, supertypes, metadata).

**The runtime's unsafety.** `tree-sitter-c2rust` is c2rust output: 22,000
lines of raw-pointer Rust, as trustworthy as the C it translates -- which is
the C every tree-sitter editor runs, fuzzed upstream. Our own `unsafe` is one
module (`gui/syntax/src/ffi.rs`), each block with its argument.

**Deviations from upstream, each documented where it is.** The scanner ports
compare a character whole where the C truncated it to a byte (a real bug:
`/* ... Ī/` ended a Rust comment), state their character classes rather than
use the C locale's, and never read or write past a buffer where the C did.

**Revisit when** a C toolchain is part of every build, including SlateOS's:
then the published `tree-sitter` crate drops into `gui/syntax/Cargo.toml` (the
dependency is already renamed to it) and the grammars can be compiled as
upstream compiles them -- the converter and the scanner ports retire.
