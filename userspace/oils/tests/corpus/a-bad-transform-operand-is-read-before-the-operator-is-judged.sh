# `${x@Z}` is not rejected by the scan that finds it. The scan's only job is to
# reach the `}`:
#
#     if (string[i] == '$' && string[i+1] == LPAREN)
#       { si = i + 2; t = extract_command_subst (string, &si, flags|SX_NOALLOC); }
#                                                     /* subst.c:1896-1902 */
#
# so `extract_dollar_brace_string` walks the whole body first — reading a
# `$( … )` on the way with a real parse — and only afterwards does
# `parameter_brace_transform` look at the operator and call it bad. Both things
# therefore happen, in that order: the failed extent is reported, and then the
# `bad substitution`.
#
# And when the extent read fails it consumes to the end of the string
# (`no_longjmp_on_fatal_error` suppressing the jump leaves `si` on the last
# byte), so the brace never closes:
#
#     value = extract_dollar_brace_string (string, &sindex, quoted, …);
#     if (string[sindex] == RBRACE) sindex++;
#     else goto bad_substitution;                     /* subst.c:9913-9918 */
#
# which is why the diagnostic names the *whole* word and why the unset case
# complains too — `${nope@Z}` is quietly empty, but `${nope@$(fi)}` reports,
# the verdict having been reached before the parameter was ever looked up.
#
# Two spellings are only stepped over rather than read, so neither reports: a
# backquote (`string_extract`, subst.c:1886) and a single-quoted run
# (`skip_single_quoted`, subst.c:1926-1938).
#
# The operand is never *expanded* — a valid operator would have been a single
# character — so a `$( … )` in one is read for its extent and never run.
#
# Verified against bash 5.2.37.

x=X
n=(1 2)
declare -A m=([k]=K)
set -- p q

echo "=== the extent is read first, then the operator is judged"
v='A${x@$(fi)}B';        echo "[${v@P}]"; echo "rc=$?"
v='A${x@$(fi)x}B';       echo "[${v@P}]"; echo "rc=$?"
v='A${x@$(fi)}B${x@$(fi)}C'; echo "[${v@P}]"; echo "rc=$?"

echo "=== the parameter is never reached, so an unset one complains too"
v='A${nope@$(fi)}B';     echo "[${v@P}]"; echo "rc=$?"
v='A${nope@Z}B';         echo "[${v@P}]"; echo "rc=$?"

echo "=== a whole-array, an element and the positionals read the same"
v='A${n[@]@$(fi)}B';     echo "[${v@P}]"; echo "rc=$?"
v='A${n[*]@$(fi)x}B';    echo "[${v@P}]"; echo "rc=$?"
v='A${@@$(fi)}B';        echo "[${v@P}]"; echo "rc=$?"
v='A${m[k]@$(fi)}B';     echo "[${v@P}]"; echo "rc=$?"

echo "=== a subscript is its own string, so it is read on its own terms"
v='A${m[$(fi)]@Z}B';     echo "[${v@P}]"; echo "rc=$?"

echo "=== a double-quoted run is read, and a here-document comes with the parse"
v='A${x@"$(fi)"}B';       echo "[${v@P}]"; echo "rc=$?"
v='A${x@$(cat <<E
hi
E
)}B';                     echo "[${v@P}]"; echo "rc=$?"

echo "=== the operand is a word, so its own brace and its quotes read as ones"
v='A${x@${x}}B';         echo "[${v@P}]"; echo "rc=$?"
v='A${x@'$'\n''}B';      echo "[${v@P}]"; echo "rc=$?"
v='A${x@$'"'"'\n'"'"'}B'; echo "[${v@P}]"; echo "rc=$?"
v='A${x@a}b{c}B';        echo "[${v@P}]"; echo "rc=$?"

echo "=== a backquote and a single-quoted run are only stepped over"
v='A${x@`fi`}B';         echo "[${v@P}]"; echo "rc=$?"
v='A${x@'"'"'$(fi)'"'"'}B'; echo "[${v@P}]"; echo "rc=$?"

echo "=== the operand is read, never expanded"
echo "A${x@$(echo Q >&2; echo Q)}B" 2>&1; echo "rc=$?"
echo "A${x@$((1+1))}B" 2>&1; echo "rc=$?"
echo "A${x@\Z}B" 2>&1; echo "rc=$?"
echo "A${x@'a'}B" 2>&1; echo "rc=$?"
echo "A${x@\"a\"}B" 2>&1; echo "rc=$?"
echo "A${x@Q U}B" 2>&1; echo "rc=$?"
echo "A${x@}B" 2>&1; echo "rc=$?"

echo "=== an empty collection is empty even with a bad operator"
e=(); echo "[${e[@]@Z}]"; echo "rc=$?"
set --; echo "[${@@Z}]"; echo "rc=$?"
set -- p q

echo "=== and the body is rebuilt from the parse, re-print and all"
f() { echo "${x@$( (echo 2) )}"; echo "${n[@]@$( (echo 3) )}"; }
declare -f f
g() { echo "${@@Z}"; echo "${x@\Z}"; echo "${m[k]@Z}"; echo "${n[*]@}"; }
declare -f g
h() { echo "${x@$(cat <<E
hi
E
)}"; }
declare -f h

echo done
