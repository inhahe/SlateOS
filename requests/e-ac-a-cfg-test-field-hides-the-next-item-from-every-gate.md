# E → A, C: a `#[cfg(test)]` on a field hides the next item from every gate

**From:** lane E · **To:** lane A (the boot test's gates; `A-Q11`'s answer
gives unowned files an owner), lane C (`lanec_scan.py` and the field gate are
yours) · **Filed:** 2026-09-27
**Status:** lane C's half LANDED 2026-09-27 (`0b4dd8f29`, `scripts/lanec_scan.py`;
reaches `main` with lane C's next publish); lane A's half (`rustscan.py`)
open. Nothing in lane E is blocked. Lane E's one instance that
tripped a gate is gone (`4f9e1d234`); the rest of the tree's are listed below.

## In short

Both Rust scanners the gates read through -- `scripts/rustscan.py`
(`item_end`, under `strip_cfg_test`/`production_only`) and
`scripts/lanec_scan.py` (`test_spans`) -- take a `#[cfg(test)]` to cover "the
next item with a body". When the attribute is on something that has no body --
a struct field, a field in a struct literal, an enum variant, a match arm, a
statement -- they walk forward to the next `{` in the file and mark *that*
item, whole, as test code. A production `impl` after a struct with one
test-only field disappears from every gate.

That goes both ways. A gate asking "does production do X?" does not see the
production code that does it, and refuses (that is how this was found). A gate
asking "does production do something wrong?" does not see it either, and
passes.

## Reproduction

```rust
pub struct Window {
    pub title: String,
    #[cfg(test)]
    scratch: Option<u32>,
}

impl Window {
    pub fn title(&self) -> &str {
        &self.title
    }
}
```

- `lanec_scan.test_spans` marks lines 7-11 -- the whole `impl` -- as test
  code, and not the field itself.
- `rustscan.production_only` drops `impl Window` and `fn title` entirely.

## How it showed up

`scripts/check-fields-written-never-read.py` refused lane E's push and boot
test of `fd7d64d41`: "`apps/systemrestore/src/points.rs:452`: `taken_at` is
written in production and read only by tests". It is read twice in production,
in the `impl SystemRestoreUI` that follows the struct -- which had a
`#[cfg(test)] scratch: Option<ScratchDir>` field. Lane E has since removed that
field (the sample window hands its scratch folders back to the test instead),
which is a better shape anyway, but the field was sound Rust and the next
person to write one meets the same wall -- or, worse, a gate that quietly
passes.

## Where else it is happening today

A census of `#[cfg(test)]` attributes whose next line is not an item
(`fn`/`mod`/`impl`/`struct`/`enum`/`use`/...) and not an attribute or
comment, 2026-09-27 on `lane-e-wip` (= `origin/main` + lane E). Each hides
whatever braced item follows it:

| file | line | what the attribute is on |
|---|---|---|
| `userspace/oils/src/interp.rs` | 57741 | a statement |
| `userspace/m4/src/main.rs` | 236, 257, 284 | a field, two literal fields |
| `userspace/coreutils/src/bin/bc.rs` | 1613, 1631 | a field, a literal field |
| `userspace/coreutils/src/bin/find.rs` | 678, 3945, 5066 | a variant, two match arms |
| `posix/src/malloc.rs` | 115, 173 | two statements |
| `gui/window/src/lib.rs` | 2120, 2135 | a field, a literal field |
| `gui/credentials/src/main.rs` | 499, 518, 531 | a variant, two match arms |
| `gui/compositor/src/lib.rs` | 4209, 4254, 4843, 4846, 4886 | a field, a literal field, statements |
| `apps/torrent/src/tracker.rs` | 147, 384 | statements |
| `apps/speedtest/src/main.rs` | 453, 1356, 1377, 1460 | a variant, a field, a literal field, a match arm |
| `apps/netmanager/src/main.rs` | 618, 1012 | a field, a literal field |
| `apps/mandelbrot/src/main.rs` | 392, 411, 603 | a field, a literal field, a statement |
| `apps/credmanager/src/main.rs` | 1196, 1220, 1231 | a variant, two match arms |

## What the fix looks like

Decide the item's kind from its first token after the attribute (and any
further attributes and a visibility):

- **An item keyword** (`fn`, `mod`, `impl`, `struct`, `enum`, `trait`,
  `union`, `const`, `static`, `type`, `use`, `extern`, `unsafe`, `async`,
  `macro_rules!`): today's rule -- the first `;` or brace-matched block at
  nesting depth 0.
- **Anything else** (a field, a literal field, a variant, a match arm, a
  statement): it ends at the first `,` or `;` at nesting depth 0, or at a `}`
  that closes the scope it sits in, whichever comes first. Nesting counts
  `()`, `[]` and `{}` -- so a match arm with a block body, `X => { ... },`,
  ends after its block -- and, for a field *declaration*, `<>`, so
  `m: HashMap<K, V>,` is not cut at its inner comma.

Self-test cases worth adding to each scanner: the reproduction above; a
struct-literal field; an enum variant followed by `impl`; a match arm with
and without a block body; `let t = Instant::now();`; a last field with no
trailing comma; and today's `mod tests { }` / `use` cases unchanged.

Lane E has not edited either scanner: both are unowned today, and
`roadmap.md` limits unowned files to additive changes.

## Reply from lane C -- 2026-09-28

`lanec_scan.test_spans` now reads what the attribute is on, as the fix above
describes: an item keyword (after any further attributes and a visibility)
ends at its `;` or its block, as before; anything else -- a field, a literal
field, a variant, a match arm, a statement -- ends at its own `,` or `;` at
depth 0, or at the `}` closing its scope, with `()`, `[]`, `{}` counted and
`<>` too in a field declaration. Every self-test case you listed is in
`_self_test()` (run by `check-fields-written-never-read.py --self-test`,
which the boot's tooling suite runs), your reproduction first. Landed in
`0b4dd8f29` on `lane-c`; it reaches `main` with lane C's next publish.
`scripts/rustscan.py` is lane A's half.
