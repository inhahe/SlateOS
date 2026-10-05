### [B] TD-OILS-AN-UNBALANCED-PAREN-IN-A-SLICES-BOUNDS-IS-AN-ARITHMETIC-ERROR-NOT-A-BAD-SUBSTITUTION

**Status:** ✅ FIXED 2026-08-14. Found 2026-08-14, measured against bash 5.2.37.
The fix turned up a second rule of the same walk, fixed with it — see "The fix"
at the end.

`skiparith` (subst.c) balances parens while looking for the colon that cuts
`${x:off:len}` in two, and an unbalanced `(` makes it run off the end. bash
then reports that as a **bad substitution** naming the whole bounds text, before
either bound is evaluated. osh implements the balancing (that is what makes
`${z:(1?2:3):1}` cut in the right place) but not the complaint, so the text
reaches the evaluator and produces an arithmetic diagnostic instead:

| written | bash | osh |
|---|---|---|
| `${z:(1}` | ``bad substitution: no closing `)' in (1`` | ``z: (1: missing `)' (error token is "1")`` |
| `${z:(1:2}` | ``… no closing `)' in (1:2`` | ``z: (1: missing `)'`` — and it cut at the colon |
| `${z:((1:2}` | ``… no closing `)' in ((1:2`` | likewise |
| `${z:1+(2}` | ``… no closing `)' in 1+(2`` | ``z: 1+(2: missing `)'`` |
| `${a[@]:(1}` | ``… no closing `)' in (1`` | arithmetic error |
| `${@:(1}` | ``… no closing `)' in (1`` | arithmetic error |

Both are rc=1, so only the message differs — but the message differs in class,
not just wording: bash's is the DISCARD-class `bad substitution` family, raised
by the cut, and it names the bounds text rather than the parameter.

Three things scope it precisely, all measured:

* It is the **whole bounds text** that is checked, once, before the cut — the
  message quotes `(1:2` entire, the colon never having split it.
* It is only the text the *cut* walks. Once a colon has been found with the
  depth back at zero, an unbalanced `(` in the length is an ordinary arithmetic
  error: `${z:0:(1}` is ``z: (1: missing `)'`` in bash too, and osh matches.
* A stray `)` at depth zero is not an error at all: `${z:)1}` is
  `)1: syntax error: operand expected` in both.

**Where:** `userspace/oils/src/parser.rs`, `slice_split_colon` — which already
tracks the depth and would only need to report a non-zero one at the end — and
its three call sites in `parse_braced_param_in`, which currently turn the
`None` that means "empty bounds" into `WordPart::BadSubst(raw)`.

**Proper fix:** `slice_split_colon` reports the unbalanced case distinctly from
the empty one, and the call sites raise ``bad substitution: no closing `)' in
<bounds text>``. That message shape already exists in
`userspace/oils/src/interp.rs` (`b"bad substitution: no closing `)' in "`,
~35600) but it names the whole *word*, whereas this one names the bounds text
only, so it needs its own carrier on the word part rather than a reuse of
`BadSubst`, whose printer names `${…}` entire.

**Blocked:** one row of the corpus case
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`,
which said so in its header and left the shape out. Now measured there.

**How it was found:** measuring bash's slice bounds exhaustively while fixing
TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND. It
was the last of four divergences that measurement turned up, and the only one
not fixed there.

**The fix (2026-08-14).** Two things, because measuring the first turned up the
second.

**(1) The complaint.** `slice_split_colon` now returns the depth it ended at
beside the split index, `parse_slice_bounds` carries a non-zero one as
`SliceBounds::unclosed`, and both `WordPart::ParamSubstr` and
`WordPart::ArraySlice` gained an `unclosed: Option<Str>` field for it. It is a
field on the operator rather than a `WordPart::BadSubst`, because *where* it is
raised is the whole of what distinguishes the two: `${z:}` is a bad
substitution even for an unset parameter, while `${u:(1}` with `u` unset is
silently empty. So the check sits exactly where the offset would have been
evaluated — `Shell::slice_bounds_unclosed`, called from `scalar_slice`,
`assoc_slice` and the indexed path of `slice_elements_resolved`, each after its
own "nothing to measure" exit. Every ordering measured lines up: an empty
array, an empty `$@`, `set -u`, and a set-but-empty scalar (which *does* report,
having one position).

`no_longjmp_on_fatal_error` — `Shell::prompt_expanding` — **suppresses** the
complaint rather than rewording it, so under `${x@P}` or `PS4` the characters go
on to the evaluator and the arithmetic error is what comes out. That is the
`if (no_longjmp_on_fatal_error == 0)` guard the report sits behind, and it is
why osh's *old* answer was right in those two contexts and only those two.

**(2) The walk is quote-aware.** Measuring (1) showed the walk steps over a
`' … '` run, a `" … "` run and a backslash-escape whole — all three counters
included, not just the paren one. `${z:"1:2"}` does not split (the evaluator
meets `1:2` as one bound and says so), `${z:1"?"2:3}` does split (the quoted `?`
buys no colon), and `${z:0"("}`, `${z:0'('}`, `${z:0\(}` and `${z:(1"("2)}` are
all balanced. osh's walk saw none of that, so before this fix it both cut in the
wrong place and complained where bash did not. Note this is about the *walk*
only: the quote characters stay in the bound, and the arithmetic reading each
half is given removes them (or does not — a `' … '` keeps its second reading).

The walk is over the text **as written**, which the same measurement pins down
from the other side: `p="("; ${z:$p 1}` and `${z:$(echo "(1")}` are ordinary
arithmetic errors, each being balanced as written however unbalanced its value.

**Verified:** 37 further rows in
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`, the
lib suite and a full sweep.
