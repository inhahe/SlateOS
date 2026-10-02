### BUG-OILS-BAD-SUBSTITUTION-CLASS. "bad substitution" had the wrong fatality and named the wrong text — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Three independent divergences in the same message:

1. **Wrong fatality, both ways.** osh treated *every* malformed `${...}` as
   fatal. bash splits them: a bad `@`-transform on a **set** parameter
   (`${x@Z}`, `${x@}`, `${x@QU}`, `${a[@]@Z}`) exits the whole (sub)shell with
   status 1, but any other malformed brace expansion (`${x!}`, `${!!}`,
   `${!$}`, `${!x*junk}`, `${#a[0]extra}`) only *discards* — the next line
   still runs. So osh truncated scripts bash keeps running:
   ```sh
   echo "${x!}"        # bash: "bad substitution", carries on
   echo "still here"   # bash prints this; osh never reached it
   ```
   (On an *unset* parameter or an empty collection there is no transform to
   reject and no error at all — `${nosuch@Z}` is simply empty.)
2. **Wrong text named.** osh printed just the offending `${...}`. bash names
   the **whole word** handed to `expand_word_internal`: `pre${x@Z}post`,
   `v=lead${x@Z}tail` → `lead${x@Z}tail` (value side only), `a"b${x@Z}"c` →
   `b${x@Z}` (a double-quoted run is re-entered as its own word, so the quotes
   are stripped), `${y:-${x@Z}}` → `${x@Z}` (the innermost operand wins). Only
   one message is emitted per word even with two bad substitutions, because
   expansion aborts at the first.
3. **Missing parameter tag on slice arithmetic.** A syntax error inside a
   substring offset/length is tagged with the parameter being sliced
   (`${x:1 z}` → `x: 1 z: syntax error…`, `${a[@]:…}` → `a[@]: …`, `${@:…}` →
   `@: …`); osh emitted the untagged form. Array *subscripts* (`${a[1 z]}`,
   `${#a[1 z]}`) and `substring expression < 0` correctly get no tag.

**Root cause.** `userspace/oils/src/interp.rs`: a single `bad_substitution()`
that always armed `unbound_error`, formatted from the local `raw` fragment with
no notion of the enclosing word. And `arith_cmd: Option<&'static str>` could
only hold a static builtin name, so there was nowhere to put a computed
parameter reference like `a[@]`. bash models the tag with a single mutable
`this_command_name` that `parameter_brace_substring` swaps for the varname.

**Fix.** `bad_substitution_with(raw, fatal)` splits into `bad_substitution` (→
`discard_error = Some(1)`) and `bad_transform_substitution` (→ `unbound_error =
Some(1)`), and returns early if an error is already pending. A new
`bad_sub_word: Option<String>` field carries the word's source text; `expand_word`,
`expand_word_pattern` and `expand_assignment_value` each gained a thin wrapper
that sets it around an `_inner` body (the wrapper had to sit on `expand_word`,
not `expand_word_annotated`, because the non-splitting branch bypasses the
latter), `expand_double_quoted` and the per-element `DoubleQuoted` fast path in
`expand_word_annotated` override it with the quote-stripped `parts_src(parts)`.
`arith_cmd` became `Cow<'static, str>` so `eval_arith_substr_bound` can install
a computed reference, and `slice_elements` gained a `star: bool` so it can name
`a[*]` vs `a[@]`. `unparse::word_src` now delegates to a new public
`parts_src`.

**Found by.** Writing the `bad-substitution.sh` differential corpus case and
probing bash 5.2.37 for each shape rather than assuming.

**Regression cover.** `bad-substitution.sh` — 7 fatal-class probes in
subshells, 5 discard-class probes each followed by a marker line that must
appear, 4 no-error-at-all probes, 6 word-naming shapes, and 10 slice/subscript
tag probes — plus the existing `param_transform_invalid_op_bad_substitution`
unit test.
