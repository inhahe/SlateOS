## `TD-C-EXPLORER-DOES-ARITHMETIC-ON-UNCHECKED-VALUES` (lane C, 2026-08-26) -- **CLOSED; entry was stale**

**Closed 2026-09-07 (lane C), on verifying it rather than on doing the work.**
`cargo clippy -p explorer --all-targets` reports **zero** arithmetic warnings,
against the 33 this entry describes.

**The check that matters is that they were fixed and not silenced**, because
this entry's own last paragraph warns that "a blanket `#![allow]` is what turns
33 known sites into an unknown number". They were fixed:

- No `arithmetic_side_effects` allow exists anywhere in `apps/explorer/src/` or
  its `Cargo.toml`; the crate takes `[lints] workspace = true`, and the
  workspace table sets the lint to `warn`.
- The lint is demonstrably live in this crate. Adding `fn _lint_canary(a:
  usize, b: usize) -> usize { a + b }` to `columns.rs` produces the warning;
  removing it returns the count to zero. (It also fired on my own code earlier
  today, on six `restored += 1` sites in `fileops.rs`.)

**The site this entry called the one that mattered is properly hardened.**
`parse_jpeg_dimensions` walks the marker chain with `checked_add` and
`saturating_add`, reads every byte through `data.get(..)?`, and takes segment
lengths through `byteread::u16_be_at` -- which is exactly the fix the entry
prescribed ("prefer `byteread`'s bounded accessors ... over hand-rolled offset
arithmetic"). The overflow that could have turned `pos + 7 > data.len()` into a
check that passes cannot occur: there is no unchecked `+` left in it.

**Nothing was done to the code for this closure.** Third stale entry closed
today, after `C-FILEDIALOG-IS-KEYBOARD-ONLY` and `C-ALARMCLOCK-SCROLLS-BY-CLIP-ALONE`
-- see the note in `todo.txt` about what that pattern is costing.

Original entry follows.

---


**In short:** the file manager has 33 places where it adds, multiplies or
subtracts without checking for overflow, and the project's own lint
(`clippy::arithmetic_side_effects`) warns about every one of them on every
build. Most are on values the program itself controls and are fine in practice;
some are on numbers that came out of a file's header, which is where this kind
of thing becomes a way to make a program misbehave with a crafted file. Nobody
has been through them to sort the two groups.

**Where it lives.** `apps/explorer/src/` — `thumbs.rs` (16 sites), `fileops.rs`
(11), `columns.rs` (6). `cargo clippy -p explorer --all-targets` lists them all.
The ones inside `parse_jpeg_dimensions` (`thumbs.rs` ~389–427) are the ones that
matter most: `pos += seg_len` and the `pos + 7 > data.len()` bounds checks walk
a JPEG's marker chain using lengths the *file* supplies, so an overflow there
turns a bounds check into a check that passes.

**Why it has not been fixed.** It predates this lane's current work and is not
caused by it; the decoder wiring added no new sites. Fixing it well means
deciding per site whether the right answer is `checked_*` and an early return,
`saturating_*`, or a comment explaining why the value cannot overflow — which is
33 small judgements, not one sweep.

**What the proper fix is.** Work the list, site by site. For the JPEG parser,
prefer `byteread`'s bounded accessors (already a dependency, already used by the
BMP path) over hand-rolled offset arithmetic; for the layout code in
`columns.rs`, `saturating_*` is almost always right because a widget position
that saturates draws wrong and a widget position that wraps draws somewhere
absurd. Where a value genuinely cannot overflow, say so in a comment rather than
allowing the lint file-wide — a blanket `#![allow]` is what turns 33 known sites
into an unknown number.

**How you would notice.** You would not, as a user, until a malformed JPEG in a
directory made the file manager read the wrong bytes. As a developer, every
`cargo clippy -p explorer` prints 33 warnings, which is enough noise to hide the
34th when someone adds it.
