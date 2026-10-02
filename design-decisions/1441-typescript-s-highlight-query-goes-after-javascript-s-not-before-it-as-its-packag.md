## 1441. TypeScript's highlight query goes after JavaScript's, not before it as its package lists them

**Date:** 2026-09-28 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** TypeScript's colouring rules are JavaScript's plus TypeScript's
own -- types, parameters, TypeScript's keywords. Its package says to read
TypeScript's rules first and JavaScript's after them; but where two rules
colour the same piece of code the later one wins -- in tree-sitter's own
highlighter as in the editor's (§1438) -- so JavaScript's general rules
("every name is a variable", "`<` is an operator") would override
TypeScript's specific ones ("this name is a parameter", "this `<` opens a
type's arguments"). Every parameter would look like any other variable.
The editor reads JavaScript's rules first and TypeScript's after them, so
the specific wins -- the order JavaScript's own package gives its own
parameter rules.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **JavaScript's query first, TypeScript's after** (chosen; TSX: JavaScript's, its JSX query, then TypeScript's) | each specific rule wins over the general one, as its author meant: parameters are parameters, a type's `<...>` brackets, and in TSX a tag a tag | departs from the list in the package's `tree-sitter.json`; a capitalised name such as `Foo` in `new Foo()` takes TypeScript's colour, a type, where JavaScript's rule makes it a constructor |
| The package's order, TypeScript's first | exactly as published | parameters coloured as plain variables, the `<` of `Map<K, V>` as an operator; in TSX, JSX's rules before JavaScript's, so every tag a variable |
| Edit the queries so no two rules overlap | either order would do | our own copies of upstream's queries, to be redone by hand at every update |

**Why the package lists them the other way.** When tree-sitter's
highlighter let the *first* rule win (§1438), TypeScript's first was the
specific-over-general order. The highlighter changed; the list did not; and
the package ships no highlight tests of its own that would have shown it.
JavaScript's package, which does, lists its general query before its
specific ones (`highlights.scm`, then `highlights-jsx.scm` and
`highlights-params.scm`).

**How it is known to be right.** `typescripts_query_goes_after_javascripts`
(`gui/syntax/src/highlighter.rs`) colours a TypeScript function in both
orders: this one paints its parameter -- at its declaration and at its use
-- as a parameter and a type argument's `<` as a bracket; the package's
order paints them a variable and an operator. `tsx_is_coloured_with_jsx_and_types`
checks TSX's tags, attributes, parameters and types.

**Revisit if** tree-sitter-typescript reorders its list, or ships highlight
tests: then its own tests say what it means, and the order follows them.
