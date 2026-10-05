### BUG-OILS-EXPANSION-ERROR-FATALITY. Non-fatal word-expansion errors killed the whole shell — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** A single mistyped array subscript, or an indirect expansion through
a value that is not a valid variable name, silently truncated the rest of the
script. osh exited the whole shell where bash discards only the offending
command and the rest of its parse unit:
```sh
a=(1 2 3)
a[-9]=v            # bash: "a[-9]: bad array subscript", carries on
echo "still here"  # bash prints this; osh never reached it
```
The same for `${#a[-9]}` (length form), `${!ptr}` with `ptr='not a name'`, and
`${arr[@]=y}` on an unset array. This is the same class of bug as the previously
fixed `break N` escape: the script *looks* like it ran to completion.

**Root cause.** `userspace/oils/src/interp.rs` had a single `unbound_error:
Option<i32>` signal that the simple-command driver turned into
`Flow::Exit(fatal_abort_status(code))` — correct for the three genuinely fatal
expansion errors (`set -u` unbound → 127, `${var:?}` → 127, bad substitution →
1) but wrong for everything else, which had been routed there too. The
non-fatal `Flow::Discard` path existed but was reachable only from a bare
`arith_error: bool` flag that could not carry a status — a problem because
`${a[@]=v}` reports **2**, not 1.

**Fix.** Split the two signals cleanly. `unbound_error` is now documented as
fatal-only (with an explicit warning that misrouting into it is a
script-truncation bug), and `arith_error: bool` became `discard_error:
Option<i32>` carrying the status bash reports. Every non-fatal site was
reclassified onto it; the four consumers (command-word expansion, the
bare-assignment branch, prefix-assignment expansion, builtin dispatch) take the
code and return `Flow::Discard` with `$?` set to it. `decl_array_ctx` was
renamed `decl_builtin_ctx` and additionally suppresses the discard for a bad
subscript given as a `declare`/`local` operand, where bash only fails the
builtin and lets the rest of the *same line* run.

**Found by.** Writing the `expansion-errors.sh` differential corpus case, and
verifying every claim against bash 5.2.37 rather than assuming.

**Regression cover.** `expansion-errors.sh` (every probe is followed by a line
that must appear), plus the unit tests
`bad_array_subscript_discards_only_the_parse_unit`,
`assoc_all_elements_subscript_is_a_literal_key_when_assigning` and
`declare_p_quotes_associative_keys_like_bash`.

**Two further divergences found and fixed alongside it.**
`${m[@]:=v}` / `${m[*]=v}` on an *associative* array assigns the literal key
`@` / `*` (a subscript there is a string key); only an indexed or undeclared
array rejects it. And `declare -p` quotes an associative key that is a lone `@`
(bash's `ALL_ELEMENT_SUB` case) or that *starts with* `#` or `~`, which osh
emitted bare.
