### TD-OILS-BRACE-BACKSLASH. A brace char-range spanning `\` (U+005C) yields a literal `\` element; bash yields an empty element there — INTENTIONAL / documented — 2026-07-20

**Where:** `userspace/oils/src/brace.rs` — `sequence_of`, single-character range
generation.

**What:** A brace character range that crosses code point U+005C (`\`) — e.g.
`{A..z}`, `{Y..a}`, `{[..]}` — includes the backslash position. osh emits a
literal `\` for that element; bash emits an **empty** string. `echo {A..z}` thus
shows `… Z [ \ ] ^ …` in osh vs `… Z [  ] ^ …` (blank where `\` would be) in
bash. Both shells agree on element *count* (58 for `{A..z}`); only the backslash
element's content differs.

**Why bash does it:** bash applies ordinary **quote removal** to brace-expansion
output. A brace-range element that is a lone `\` is a trailing/standalone
backslash, which quote removal deletes → empty. The same re-scan also makes a
generated backtick (`` ` ``, U+0060, in-range for `{A..z}`) start a
**command substitution** when a suffix is appended (`{Y..a}Q` → bash errors
`bad substitution: no closing`` ` ``), i.e. bash re-lexes range output as source.

**Why osh's behavior is intentional:** osh treats brace-range-generated
characters as **final literal data** and does not re-lex/quote-remove them. This
is simpler, width/locale-independent, and strictly safer — it never
reinterprets a generated backtick as command substitution or a generated `$` as
an expansion. The only observable cost is this one blank-vs-`\` cell in the
astronomically rare case of a range deliberately spanning `\`. No real script
relies on it; no action needed.
