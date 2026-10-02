## TD-C-ONE-DECISION-ABOUT-PATHS-IS-IMPLEMENTED-TWICE-BYTE-FOR-BYTE -- FIXED 2026-09-16

**In short:** design-decisions §426 chose one way to write a filename into a
file that has to stay human-readable, and said in as many words that the point
was to have *one* escape rather than a different one per format. It is
implemented twice: the same four functions, byte-for-byte identical, in two
programs. They agree today and nothing keeps them agreeing.

**Date:** 2026-09-16. **Lane:** C.

**Where.**

| | |
|---|---|
| `apps/explorer/src/fileops.rs:1709-1766` | `encode_path`, `encode_bytes`, `decode_path`, `decode_bytes` |
| `apps/backup/src/main.rs:1032-1075` | the same four, identical |

**The four function bodies are identical.** The doc comments are not, and the
first version of this entry said otherwise -- "the only difference in forty-four
lines is one doc comment" -- which was wrong, and wrong in a way worth naming:
I diffed two `sed` ranges that each began at `fn encode_path`, so the leading
doc comment of each block fell *outside* what I compared. Backup's explains the
JSON manifest, explorer's explains the recycle bin's `meta.txt`. A diff of a
range chosen by line number answers a question about that range, not about the
thing you meant.

That difference is the interesting part rather than a detail. Each copy carried
a doc comment justifying it in terms of its own file format, which is precisely
what made two copies of one decision feel reasonable to whoever wrote the
second: it does not read as a duplicate, it reads as this program's own
handling of this program's own format.

**Why this is worse than ordinary duplication.** §426's own reasoning rejected
option (4) -- escaping only what each container forbids -- on the grounds that
*"each format gets a different escape with different edge cases, and 'what the
container forbids' is exactly the kind of thing that is revisited later and
gets it wrong."* The decision was made specifically to avoid two encodings.
Copying the implementation reintroduces exactly the risk the decision was taken
to remove: the recycle bin and the backup manifest can now drift apart one edit
at a time, and the first symptom would be a restore that produces a name the
other program cannot read.

**Why it happened, as far as the code shows.** Both are binaries. There was no
crate they could share that `apps/backup` was allowed to depend on -- it is a
command-line program and its manifest says outright that it *must not link a
widget library*, which rules out `guitk`, where lane C's other shared code
lives. `textfmt` exists at the repo root for precisely this layering, and the
root is not lane C's to write.

**FIXED 2026-09-16:** `apps/pathcodec`, a dependency-free crate under `apps/`
-- the position `apps/safeio` already establishes, and the only one available,
since `guitk` is ruled out by backup's must-not-link-a-widget-library rule and
the root is not lane C's to write. Both programs now use it; roughly 3,500
characters of duplicate came out of each.

Two things fell out of the move that are worth recording:

* **The code was lint-clean in both original homes and failed clippy in the new
  crate.** `out.push_str(&format!("%{b:02X}"))` allocates a three-character
  `String` per byte and throws it away; the new crate's lints object and are
  right. It now pushes the digits directly. A lint's reach is a property of
  where code *sits*, not of what it does -- so moving code can find defects
  that were always there.
* **The all-256-byte round-trip test is what made that safe.** Replacing
  `format!("{b:02X}")` with hand-rolled nibble arithmetic is exactly the edit
  that quietly produces `%A` for `%0A`, and `every_byte_round_trips` would
  fail on the first byte below 16. Writing it before the move rather than
  after was luck, but it is the reason the move is checkable at all.

**It also unblocks something else.** `apps/settings` cannot store a wallpaper
whose path is not text (**C-Q24**), and neither can the manual file order or
the backup's record of its own source. §426 is the answer to all of them and is
already decided, already implemented and already tested -- it is simply not
reachable from those crates. Extracting it turns C-Q24 from a question into a
task.
