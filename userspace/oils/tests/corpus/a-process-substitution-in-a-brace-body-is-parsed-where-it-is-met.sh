# `parse_matched_pair` names `<(`, `>(` and `$(` in one breath and sends all
# three through `parse_comsub` (parse.y:5028-5042), so a `${ … }` body's scan
# reads a process substitution exactly as it reads a command substitution: the
# body is parsed there and then, its error is the enclosing unit's — `echo
# "${z:-<(fi)}"` is a `near unexpected token 'fi'`, not a brace body holding the
# text `<(fi)` — and what survives is the parse *re-printed*, which `declare -f`
# shows.
#
# It holds wherever in the body the substitution is written and whatever the
# `${ … }` itself stands in: an operand, a pattern, a replacement, a subscript,
# quoted or bare. What it does not reach is text this scan does not read — a
# `' … '` run at the top of the body, and a `" … "` run, whose own scan has no
# such row (`echo "<(fi)"` is just the word `<(fi)`).
#
# Because the body is read here, the `)` it wants is this scan's business too:
# one that never closes runs the enclosing construct out of input, and the
# delimiter named is the one the `${` was written in — the `"` when there is
# one, the `)` otherwise. That is the same answer the `$(` spelling gives in the
# same place.
#
# Performing it is a separate question, answered at expansion time and not here:
# see known-issues.md, TD-OILS-A-PROCESS-SUBSTITUTION-IN-A-BRACE-BODY-IS-NEVER-
# PERFORMED. Every case below is one whose answer the parse alone decides, so
# nothing here prints a `/dev/fd/N`.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the body is parsed where the scan meets it ==="
e 'echo "${z:-<(fi)}"'
e 'echo ${z:-<(fi)}'
e 'echo "${z#<(fi)}"'
e 'echo "${z/x/<(fi)}"'
e 'echo "${a[<(fi)]}"'
e 'echo "${z:-a>(fi)b}"'
e 'echo "${#x:-<(fi)}"'
e 'echo "${x:-${y:-<(fi)}}"'

echo "=== left to right, beside the other spellings ==="
e 'echo "${z:-$(echo x)<(fi)}"'
e 'echo "${z:-<(fi)$(done)}"'
e 'echo "${z:-`fi`<(done)}"'

echo "=== a run this scan does not read keeps its characters ==="
e 'echo "${z:-'"'"'<(fi)'"'"'}"'
e 'echo "<(fi)"'
e 'echo '"'"'<(fi)'"'"''
e 'echo $(( <(fi) ))'

echo "=== the ) is this scan's to want, in the delimiter the \${ stands in ==="
e 'echo "${z:-<(fi}"'
e 'echo ${z:-<(echo hi}'
e 'echo "${z:-<(echo }hi)}"'

echo "=== the re-print is what stands, not the source ==="
f() { echo "${z:-<(echo   hi)}"; }; declare -f f
g() { echo "${z:-<(if true; then echo a; fi)}"; }; declare -f g
h() { echo "${z:-<( (echo a) )}"; }; declare -f h
echo "${z:-<(echo $(echo q))}"

echo "=== numbered from the <( , on the physical line ==="
e 'echo one
echo "${z:-<(
fi)}"'
e 'echo one
echo "${z:-$(
fi)}"'
echo TAIL
