### TD-OILS-EXTGLOB-PARSE. `osh` always *parses* extended-glob syntax (`+(…)` etc.) even when `extglob` is off; bash gates it at parse time — ✅ RESOLVED 2026-07-28 (superseded by TD-OILS-EXTGLOB-UNGATED)

**Where:** `userspace/oils/src/parser.rs` / `lexer.rs` (word/case-pattern
lexing always recognises `?(`/`*(`/`+(`/`@(`/`!(` groups), vs `interp.rs`
`exec_case` (line ~1368) which honours the `extglob` flag at *match* time.

**What:** in bash, the `extglob` shopt affects **parsing** of extended patterns
in `case` arms and globs — with `extglob` off, `case foo in +(f|o))` is a
*syntax error* (`unexpected token '('`). `osh`'s parser always accepts extended
pattern groups, then `case`/glob matching honours the runtime `extglob` flag
(off ⇒ the group is treated literally, so `+(f|o)` fails to match `foo`,
yielding the `*)` arm). Net effect: `osh` accepts and runs some scripts bash
would reject at parse time when `extglob` is off; it never *mis-matches* (a
pattern that bash would match with extglob on, osh only matches when the flag is
on too). This is strictly more lenient, not wrong-answer-producing.

**Note:** this is distinct from — and does *not* apply to — `[[ str == pat ]]`,
where bash matches "as if extglob were enabled" unconditionally; `osh` now does
the same (fixed 2026-07-19, `cond_binary`, test `dbracket_match_always_uses_extglob`).

**Why deferred / accepted:** matching bash's parse-time gating would require
threading the *runtime* `extglob` state into the parser (parsing happens before
any `shopt` on the same input runs — which is exactly why bash rejects a
same-line `shopt -s extglob; case … +( …`). The leniency is defensible and
harmless in practice (real scripts enable `extglob` in a prior file/line before
the pattern is parsed), so this is documented rather than "fixed" by making the
parser stricter. **Proper fix (if ever wanted):** have the lexer treat `X(` as an
extended group only when a parse-time `extglob` flag is set, and surface a syntax
error otherwise — but this buys only bug-for-bug parity on invalid input.

**Resolved 2026-07-28.** The "why deferred" reasoning above turned out to be
backwards: the leniency is *not* harmless, because with extglob off bash reads
`!(cmd)` as a **negated subshell** — `if !(false); then …` is ordinary shell —
and a function name may legitimately end in a glob character (`f?() { …; }`).
osh swallowed both as pattern groups. And threading parse-time state was already
architecturally available, since `run_source_flow_out` executes unit-by-unit and
re-reads alias state per unit. See TD-OILS-EXTGLOB-UNGATED above for the fix.
