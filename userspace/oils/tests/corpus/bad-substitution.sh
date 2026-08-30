# "bad substitution" is really *two* errors wearing one message, and telling
# them apart matters because they end different amounts of the script:
#
#   * a bad `@`-transform on a *set* parameter — `${x@Z}` — is FATAL: the whole
#     (sub)shell exits 1, so everything after it in that shell is skipped;
#   * every other malformed `${...}` — `${x!}`, `${!!}`, a leftover after a
#     length form — merely DISCARDS: the command never runs, `$?` is 1, the
#     rest of that parse unit is abandoned, and the next line still runs.
#
# A shell that picks the wrong class either truncates a script that bash keeps
# running or keeps running one bash abandoned. Each probe below is followed by
# a marker, so the class is visible in the diff.

x=v
a=(1 2 3)
declare -A m=([k]=1)
set -- p q r

echo "=== fatal class (each in its own subshell) ==="
( echo "${x@Z}"; echo "unreachable-1" ); echo "st=$?"
( echo "${x@}"; echo "unreachable-2" ); echo "st=$?"
( echo "${x@QU}"; echo "unreachable-3" ); echo "st=$?"
( echo "${a[0]@Z}"; echo "unreachable-4" ); echo "st=$?"
( echo "${a[@]@Z}"; echo "unreachable-5" ); echo "st=$?"
( echo "${m[k]@Z}"; echo "unreachable-6" ); echo "st=$?"
( echo "${@@Z}"; echo "unreachable-7" ); echo "st=$?"

# On an *unset* parameter — or an empty collection — there is no transform to
# reject, so nothing is an error at all and the value is simply empty.
echo "unset=[${nosuch@Z}] st=$?"
unset -v empty_arr
echo "emptyarr=[${empty_arr[@]@Z}] st=$?"
declare -A empty_map
echo "emptymap=[${empty_map[@]@Z}] st=$?"
set --
echo "noargs=[${@@Z}] st=$?"
set -- p q r

echo "=== discarding class (top-level shell survives) ==="
echo "${x!}"; echo "unreachable-8"
echo "after-bang=$?"
echo "${!!}"; echo "unreachable-9"
echo "after-bangbang=$?"
echo "${!$}"; echo "unreachable-10"
echo "after-bangdollar=$?"
echo "${!x*junk}"; echo "unreachable-11"
echo "after-prefixjunk=$?"
echo "${#a[0]extra}"; echo "unreachable-12"
echo "after-lenjunk=$?"
echo "still-alive"

echo "=== which word gets named ==="
# The diagnostic names the *whole word* handed to the expander, not just the
# offending `${...}`: adjacent literals are part of it.
( echo "pre${x@Z}post" ) 2>&1 >/dev/null; echo "st=$?"
( echo pre${x@Z}post ) 2>&1 >/dev/null; echo "st=$?"
# …but enclosing double quotes are stripped: a quoted run is re-entered as a
# word of its own, so only the quoted section's contents are named.
( echo a"b${x@Z}"c ) 2>&1 >/dev/null; echo "st=$?"
# An assignment names only the value side.
( v=lead${x@Z}tail; echo "unreachable-13" ) 2>&1 >/dev/null; echo "st=$?"
# The innermost operand wins — the nested word is expanded on its own.
( echo "${nosuch:-${x@Z}}" ) 2>&1 >/dev/null; echo "st=$?"
# Expansion stops at the first error, so a word with two bad substitutions
# still reports exactly one message.
( echo "${x@Z}${x@Y}" ) 2>&1 >/dev/null; echo "st=$?"

echo "=== substring/slice arithmetic carries the parameter reference ==="
# A syntax error inside a *substring* offset/length is tagged with the
# parameter being sliced — the tag is the reference in source form.
s=abcdef
echo "${s:1 z}"; echo "unreachable-14"
echo "after-scalar=$?"
echo "${s:0:1 z}"; echo "unreachable-15"
echo "after-scalar-len=$?"
echo "${a[@]:1 z}"; echo "unreachable-16"
echo "after-at=$?"
echo "${a[*]:1 z}"; echo "unreachable-17"
echo "after-star=$?"
echo "${a[0]:1 z}"; echo "unreachable-18"
echo "after-elem=$?"
echo "${@:1 z}"; echo "unreachable-19"
echo "after-pos-at=$?"
echo "${*:1 z}"; echo "unreachable-20"
echo "after-pos-star=$?"
# An array *subscript* is not tagged — only the slice bounds are.
echo "${a[1 z]}"; echo "unreachable-21"
echo "after-subscript=$?"
echo "${#a[1 z]}"; echo "unreachable-22"
echo "after-len-subscript=$?"
# …and neither is a negative-length complaint.
echo "${s:0:-99}"; echo "unreachable-23"
echo "after-negative=$?"

echo "done"
