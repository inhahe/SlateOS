### TD-OILS8. `osh` extglob: `!(cmd)` with no space is a pattern word, not a negated subshell — OPEN (low priority, intentional superset tradeoff)

**Where:** `userspace/oils/src/lexer.rs` (`read_word`, the extglob group
opener) and the whole-program parse entry (`parse`).

**What:** `extglob` extended-pattern matching (`?()`/`*()`/`+()`/`@()`/`!()`)
is fully implemented in the matcher (`compile_glob`/`match_glob_toks`) and
honored in pathname expansion, `case`, `[[ == ]]`/`[[ != ]]`, and
parameter-expansion pattern operators. Bash gates the *lexing* of these
groups on `shopt -s extglob` being in effect **at parse time**; our
interpreter parses the whole program up front, independently of runtime
`shopt`, so the lexer always consumes an unquoted `X(…)` group (X ∈
`?*+@!`) into a single word token and defers the extglob decision to match
time (with extglob off the group matches literally, which is bash-correct
for the common patterns). The one visible consequence: `!(cmd)` written
with **no space** is now lexed as a pattern word rather than a `!`-negated
subshell. Workaround: write `! (cmd)` (with a space) for a negated
subshell — which is the more common and readable form anyway.

**Proper fix (if ever needed):** thread an `extglob` flag through the
parser that mirrors bash's parse-time gating — e.g. re-parse or lazily
tokenize on a per-command basis so a `shopt -s extglob` earlier in the
script enables group lexing only for later commands. Deferred: the current
behavior is a deliberate, documented superset tradeoff and `!(cmd)`
no-space negated subshells are vanishingly rare.
