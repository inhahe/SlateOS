### TD-OILS-BRACE-BACKSLASH. Brace char-range spanning the backslash char (e.g. `{A..z}`) — `osh` emits a literal `\`, bash drops it — MINOR, obscure 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (brace-expansion char-sequence
generator). A char range like `{A..z}` walks ASCII code points A(65)..z(122),
which crosses `\` (92).

**What:** bash's brace expansion is a *textual* rewrite of the raw token, so the
generated word for code point 92 is a single backslash that then undergoes quote
removal — the backslash escapes nothing and is removed, yielding an **empty**
field. `osh` builds brace results as already-parsed literal words (no post-hoc
quote removal), so it emits a literal `\` field instead. Concretely
`printf "[%s]" {A..z}` shows `…[[][][]]…` in bash (empty field between `[` and
`]`) but `…[[][\][]]…` in osh. Only ranges that straddle char 92 differ; every
other range matches. No real script relies on this.

**Why deferred:** matching bash needs brace expansion to run as a textual pass on
raw token bytes with the result re-fed through quote removal, which is a
different architecture than osh's structured expansion. High effort, essentially
zero practical value (a range crossing the backslash char is a pathological
input). **Proper fix (if ever wanted):** apply quote-removal semantics to
brace-generated literal segments so a generated lone `\` is consumed as an
escape, matching bash.
