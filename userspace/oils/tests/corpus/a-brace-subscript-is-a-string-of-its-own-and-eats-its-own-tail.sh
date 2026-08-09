# A `${…}` written inside a `${name[…]}` subscript is not part of the word the
# brace sits in — the subscript is handed to a **separate** expansion call, and
# that call has its own `sindex`. Two consequences fall out of the one fact, and
# `${x@P}` is the only place either is visible (everywhere else the failed
# `$( … )` parse jumps out of the command entirely; see
# `a-runtime-subscript-is-unread-text-and-reads-its-own-comsub.sh`).
#
# **What the report names.** `expand_word_internal` reaches a `$(` and calls
# `extract_command_subst`, which scans forward from there to the *end of the
# string it was given*. Inside a subscript that string is the subscript, not the
# word: `array_expand_index` copied it out with `strncpy (exp, s, len-1)`
# (arrayfunc.c:1339-1358) before ever calling `expand_arith_string`. So
# `b='A${a[p$(fi)q]}B'` reports `` `fi)q' `` — the subscript's remainder — and
# not `` `fi)q]}B' ``. The second report is one byte shorter (`` `fi)' ``)
# because `xparse_dolparen` returns `*indp = ep - base - 1` (parse.y:4349) and
# the retry re-reads from there.
#
# **What the failure consumes.** The read left `sindex` at the end of *its*
# string, so `expand_word_internal` simply ends — everything after the `$( … )`
# in that string contributes nothing, literals and further expansions alike.
# But it is the subscript's string that ended, so the word around it carries on
# normally: `A${a[$(fi)]}B${a[1]}C` is `A0B1C`, the empty arithmetic string
# evaluating to 0. For an *associative* subscript the empty text is an empty
# key, which is bash's "bad array subscript" — so the abandoned read is what
# makes `A${m[$(fi)qq]}B` report it and expand to `AB`.
#
# A backquote is not scanned this way: `string_extract` only steps over its own
# extent, so its body is read on its own terms and nothing after it is lost.
#
# Verified against bash 5.2.37.

a=(0 1 2)
declare -A m=([k]=K)
s=abcdef

echo "=== the report names the subscript's remainder, not the word's"
b='A${a[p$(fi)q]}B'; echo "[${b@P}]"

echo "=== the failure consumes the subscript and stops there"
c='A${a[$(fi)]}B${a[1]}C'; echo "[${c@P}]"
d='A${a[$(fi)xy]}B'; echo "[${d@P}]"
e='A${a[$(fi)]}${a[1]}${a[2]}Z'; echo "[${e@P}]"

echo "=== two subscripts in one word are two strings"
f='A${a[$(fi)x]}B${a[$(fi)y]}C'; echo "[${f@P}]"

echo "=== an empty associative key is a bad subscript"
g='A${m[$(fi)qq]}B'; echo "[${g@P}]"
h='A${m[$(fi)]}B${m[k]}C'; echo "[${h@P}]"

echo "=== a backquote is only stepped over"
i='A${a[`fi`]}B'; echo "[${i@P}]"

echo "=== a length asks the same subscript"
j='A${#a[$(fi)q]}B'; echo "[${j@P}]"

echo "=== the whole word is the string when no subscript owns it"
k='A$(fi)B${a[1]}C'; echo "[${k@P}]"
l='A${a[1]}B$(fi)C'; echo "[${l@P}]"

echo "=== a slice bound is read with the brace, so the brace itself is bad"
n='A${s:$(fi)q:2}B'; echo "[${n@P}]"

echo done
