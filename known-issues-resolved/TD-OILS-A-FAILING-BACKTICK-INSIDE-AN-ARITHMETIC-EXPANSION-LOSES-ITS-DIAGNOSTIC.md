### TD-OILS-A-FAILING-BACKTICK-INSIDE-AN-ARITHMETIC-EXPANSION-LOSES-ITS-DIAGNOSTIC. `` echo $(( 1 + `fi` )) `` printed only the arithmetic error — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — the arithmetic evaluator's expansion
of a command substitution inside an expression.

**What.** A backtick *is* opaque to the arithmetic scan, so unlike the entry
above it survives to expansion time — and then its body is parsed, fails, and
bash reports it before going on to fail the arithmetic on the empty result. osh
reports only the second half:

```text
bash: command substitution: line 1: syntax error near unexpected token `fi'
      command substitution: line 1: `fi'
      line 1: 1 +  : syntax error: operand expected (error token is "+  ")   rc=1
osh :  line 1: 1 +  : syntax error: operand expected (error token is "+  ")  rc=1
```

The arithmetic error itself matches byte for byte, including the two spaces
where the substitution's empty value went — so the expansion happens and the
value is right; only the body's own diagnostic is swallowed.

**Fixed by** two changes to `Shell::run_command_sub_text`, the entry point the
arithmetic evaluator uses for a substitution it carved out of an expression
string. The second was invisible until the first landed.

- **The eager `parse` pre-check is gone.** The function opened with
  `if crate::parser::parse(text).is_err() { return Str::new(); }`, whose comment
  reasoned that a body the enclosing parse never saw could not be attributed
  anywhere, so a diagnostic would be blamed on the wrong place. Measurement
  contradicted it: bash blames the body exactly as it blames a substitution in
  an ordinary word — `command substitution: line N: …` — and only then fails
  the arithmetic on the empty value the failure left behind. Nothing needed to
  be *added* to report it; `Shell::command_sub` already runs the text through
  the read-eval loop with `comsub_read_eval` set, and the pre-check was the only
  thing standing in its way. Deleting it also corrected an exit status for
  free: `` x=$(( `fi` )) `` returned 0 and now returns bash's 2.
- **The body is numbered off the line the enclosing command reached**, not from
  its own line 1. The map was `LineMap::Offset(0)`; it is now
  `LineMap::Offset(self.current_line.saturating_sub(1))` — the same rebasing
  `Shell::command_sub_body_inner` gives a `CmdSubBody::Backtick` in a word.
  bash's rule, measured: blamed line = (line the enclosing command reached) +
  (line within the body) − 1, so a body whose `fi` sits on physical line 6 with
  the closing backtick on line 7 is blamed at **line 8** — past the whole
  construct. `current_line` is already absolute, so an arithmetic expression
  inside an `eval` or a function composes without further work.

**Still divergent, tracked separately:** `` declare -i n; n=1+`fi` `` — see
TD-OILS-A-FAILING-SUBSTITUTIONS-STATUS-IS-LOST-WHEN-AN-INTEGER-ASSIGNMENT-ALSO-FAILS.

Covered by `tests/corpus/a-backtick-that-fails-to-parse-is-blamed-before-the-arithmetic-is.sh`.
