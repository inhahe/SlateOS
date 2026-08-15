# `extract_dollar_brace_string` (subst.c:1874-1960) is a **single forward pass
# that reports as it goes**, and its ending is the last thing it does. Its `$(`
# row (subst.c:1896) hands the *rest of the string* to `extract_command_subst`
# → `xparse_dolparen` — a real parse, whose diagnostic is printed there and
# then — and reads the reader's position back out of it (`*sindex = si`,
# subst.c:1322-1330). Only when the walk finally runs out of text does
# `if (c == 0 && nesting_level)` raise the `bad substitution`.
#
# So the two are ordered and **both** happen: every read the scan passed on the
# way has already reported by the time the brace is condemned.
#
# The position it reads back is the whole of the difference between the two
# endings, and it is not a property the scan can work out for itself:
#
# * the parse **errors part way** down its text — `si` lands mid-string, the
#   brace scan resumes there and can still find its `}`, so the substitution
#   expands as though nothing were wrong;
# * the parse **runs the string out** — `si` is past the end, the brace has
#   nothing left to close on, and the word is condemned undecoded.
#
# Nothing between the braces is ever *run*, either way. That is the one place
# this rule parts company with the string level, where the same failure runs
# what it read less a byte and splices the rest (`A$(for⏎sB` below).
#
# The `<(`/`>(` spellings are the same row: `extract_command_subst` does not
# know which delimiter sent it. `$[ … ]` is **not** — its
# `extract_arithmetic_subst` passes flags `0` (subst.c:1299), so it makes no
# nested read at all.
#
# All of this needs `no_longjmp_on_fatal_error` to be visible: under `${x@P}`
# the parse failure reports without jumping, so the scan carries on to its own
# ending. Anywhere else the first failure takes the jump and there is no second
# act.
#
# Verified against bash 5.2.37.

z=ZZ

echo "=== the string level, for contrast: what was read is run ==="
v='A$(for
sB'; printf '[%s]\n' "${v@P}"

echo "=== a read that stops part way leaves the brace its }  ==="
v='A${z:-pr$(for
s)q}B'; printf '[%s]\n' "${v@P}"
v='A${z:-P1$(for
S1}B'; printf '[%s]\n' "${v@P}"
v='A${z:-P1<(for
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== …and a read that runs the string out takes the } with it ==="
v='A${z:-P1$(echo hi
S1}B'; printf '[%s]\n' "${v@P}"
v='A${z:-$(echo hi'; printf '[%s]\n' "${v@P}"

echo "=== the report comes first either way ==="
v='A${z:-P1$(for
S1B'; printf '[%s]\n' "${v@P}"
v='A${z:-P1$(echo hi
S1B'; printf '[%s]\n' "${v@P}"

echo "=== every read is reported, not just the first ==="
v='A${z:-p$(fi
q)r$(for
s)t}B'; printf '[%s]\n' "${v@P}"
v='A${z:-p$(fi
q)r$(for
s}B'; printf '[%s]\n' "${v@P}"
v='A${z:-p$(fi
q)r${w'; printf '[%s]\n' "${v@P}"

echo "=== an unclosed brace with no read in it has nothing to report ==="
v='A${z:-pr
sB'; printf '[%s]\n' "${v@P}"

echo "=== nothing between the braces is run ==="
v='A${z:-P1$(echo in >&2
S1}B'; printf '[%s]\n' "${v@P}"
v='A${z:-$(echo one >&2)p$(fi)q}B'; printf '[%s]\n' "${v@P}"

echo "=== a body that closes is unmoved ==="
v='A${z:-P1$(echo hi
q)}B'; printf '[%s]\n' "${v@P}"
v='A${z:-pr`fi`q}B'; printf '[%s]\n' "${v@P}"

echo TAIL
