## `apps/editor`'s syntax highlighter is complete, tested, and not connected (lane C)

**Status: FIXED 2026-08-16** (lane C). The highlighter now draws the editor.
`render_editor` tokenizes each visible line and emits one `tree.text` per token
in the token's theme colour, and the `HighlightState` entering the first visible
line comes from `Document::entry_state`, a per-line memo (`hl_entry:
RefCell<Vec<HighlightState>>`) that every mutating operation truncates from the
first line it touched. The memo is what makes a block comment opened on line 3
colour line 4000 without re-tokenizing 4000 lines every frame. `#![allow(dead_
code)]` is gone from `highlight.rs`, and `detect_language` became
`language_of_path`, the one place an extension maps to a `Language`. Seven new
tests in `main.rs`'s `highlight_render_tests` cover it, the load-bearing one
being `the_syntax_cache_agrees_with_a_recomputation_after_every_edit`: it runs
all ten editing operations and asserts the cached entry state equals a
from-scratch recomputation, which is the assertion that fails if someone later
adds an edit path and forgets to invalidate. 97 tests pass, clippy clean.

*Original report follows.*

**Status: OPEN 2026-08-15** (lane C). Found while sweeping for byte-at-a-time
text walkers — `highlight.rs`'s tokenizers step bytes, so it was on the list to
audit, and the audit turned up something larger: the module is not reachable
from the running program.

`apps/editor/src/main.rs` declares `mod highlight;` and then never names
anything from it. There is no `use crate::highlight::…`, no `highlight::`
path anywhere outside the module, and `highlight_line` — the only public entry
point — is called exclusively from `highlight.rs`'s own `#[cfg(test)]` block.
That is 2328 lines implementing Rust, Python, C, JavaScript and Markdown
tokenizers, with a large and genuinely passing test suite, wired to nothing.
The editor's own module doc still advertises "Syntax highlighting for common
languages".

**Why the compiler did not catch it:** `highlight.rs` line 11 is
`#![allow(dead_code)]`. Without it, every public item in the module would be
reported unused in this binary crate. The suppression is doing real damage
here — it is the only thing standing between this state and a build warning.

**What the proper fix looks like.** Wire `highlight_line` into the text
rendering path so a line is drawn as a run of per-token coloured spans instead
of one uniform string, threading `HighlightState` down the visible-line loop so
multi-line constructs (block comments, multi-line strings) carry over — the
state machine for that already exists and is tested. Then delete the
module-level `#![allow(dead_code)]` and fix whatever it was masking. Check
`syntree.rs` is unaffected: it *is* wired up (`use syntree::{Pos, SyntaxTree}`),
so this is specific to highlighting.

**The byte-stepping was audited first, and it is clean — this prerequisite is
already done.** The tokenizers scan `line.as_bytes()` and advance one byte in
their default branches, so a token boundary landing mid-character would panic
the moment a caller sliced by the range, and a renderer would slice on every
keystroke. `no_tokenizer_splits_a_character` now runs all twelve languages over
non-ASCII source — CJK, Greek, Cyrillic and emoji inside string literals,
comments and bare identifiers — and asserts `is_char_boundary` on both ends of
every token before slicing; `multi_line_state_survives_non_ascii` covers a
block comment carried across three lines, the path where a tokenizer resumes at
an offset it did not choose. Both pass: every boundary really is decided by an
ASCII delimiter, so UTF-8 self-synchronization holds throughout. Verified
non-vacuous by making one `push_token` emit `len - 1`, which the assertion
catches. So wiring highlighting up is now purely a rendering job, with no
character-boundary work left to do.


Running list of unsolved bugs and technical debt.  Each entry should
have enough context to act on later: what the bug or debt is, where in
the code it lives, how to reproduce it (for bugs), and what the proper
fix looks like (for debt).

Per CLAUDE.md: "Ideally, bugs and tech debt are fixed immediately as
they're discovered — the tracking file is a fallback for when something
genuinely can't be addressed in the current task, not a place to defer
work that should be done now."
