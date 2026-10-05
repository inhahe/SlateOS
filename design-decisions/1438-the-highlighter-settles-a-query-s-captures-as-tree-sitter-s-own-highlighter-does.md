## 1438. The highlighter settles a query's captures as tree-sitter's own highlighter does

**Date:** 2026-09-28 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** A language's colouring rules (its *highlight query*: patterns
that pick out pieces of the parsed code and name what they are) often say
two things about the same text -- "every name is a variable", then "a name
being called is a function". Which one wins decides whether code comes out
coloured as the rules' authors meant. The code editor now settles this the
way tree-sitter's own highlighter does, which is what those authors test
their rules against: the later rule wins, and rules that start at the same
place stack in the order they are written. The editor had kept the *first*
rule, so every YAML key came out as a string and a Rust method call as a
field. The grammars' own colouring tests now run here and pass.

**The rules, each as tree-sitter-highlight 0.25 has them:**

1. **Of one node's captures, the last pattern's wins.** A query lists the
   general before the specific: Rust's has `(field_identifier) @property`
   long before the method-call pattern that calls the same node
   `@function.method`; YAML's makes every scalar `@string` before it makes a
   key `@property`. (Tree-sitter's highlighter once kept the first; the
   vendored queries are written for the last.)
2. **Captures stack in the order they come**, by start and then by pattern:
   a later pattern's capture that starts where an earlier one does goes on
   top for its whole length, longer or shorter, and what is under it stays
   hidden until it closes. TOML's query relies on it: `(pair (bare_key))
   @property` -- the whole pair -- over `(bare_key) @type`, to make a key a
   property. Flattening by containment instead (the shorter on top, which
   the editor did) coloured every TOML key as a type.

**Where it departs from tree-sitter's highlighter, on purpose:**

| Question | Chosen | tree-sitter-highlight | Why |
|---|---|---|---|
| A capture whose name has no colour here (`@spell`, `@text.emphasis`) | takes no part; the node keeps what the other patterns said | if it is the node's last, the node is left uncoloured | we colour 22 kinds, not a theme's hundred names; blanking a node because its last name is one we lack (nvim-style `(comment) @comment @spell`) would lose colour the query did give it |
| `@none` | paints the text plain, over what encloses it | a name like any other (no colour unless a theme has it) | Neovim's meaning, which the Markdown query (from nvim-treesitter) uses to keep a code fence's contents from showing the fence's literal colour |
| An injected language's span starting where its host's does | the injected one on top | the host's on top, but an injection wins an identical range | an embedded language's colours are the point of injecting it; Neovim draws injected trees over their hosts |
| What an injection's text leaves out, without `injection.include-children` | the node's *named* children only | every child | Markdown's block grammar lexes a paragraph's backticks and brackets as anonymous tokens of the paragraph's `inline` node; leaving them out handed the inline grammar fragments, and nothing inline was coloured. Named-only is Neovim's reading, whose queries these are (Helix spells it `injection.include-unnamed-children`) |

**How it is known to be right.** Five of the vendored grammars ship
highlight tests (`test/highlight/`: source files whose comments point at the
line above -- `// ^ function` -- and name its capture), which upstream runs
with `tree-sitter test` against tree-sitter-highlight. They are vendored
under `gui/syntax/grammars/<name>/highlight/` and run against this
highlighter (`gui/syntax/src/highlight_tests.rs`), their assertions read as
`tree-sitter test` reads them and compared as the kinds the names paint: C,
CSS, Python, TOML and YAML, 135 assertions, all passing. The
first-pattern rule fails seven of them, in four of the five languages;
flattening by containment fails TOML's key.

**Revisit if** tree-sitter's highlighter changes its rule again (its source
says what it does in the loop after "Once a highlighting pattern is found
for the current node"), or if the toolkit gains a theme with capture names
of its own, when the second table's first row should follow upstream.
