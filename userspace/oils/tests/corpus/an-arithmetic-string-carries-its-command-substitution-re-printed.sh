# An arithmetic scan does not keep the source of a `$( … )` it stepped over.
# `parse_matched_pair` under `P_ARITH` sends one to `parse_dollar_word` and from
# there to `parse_comsub` (parse.y:3931, 3959), and what `APPEND_NESTRET` splices
# back into the scan's buffer is `print_comsub (parsed_command)` — the parse
# re-printed (parse.y:4219–4241). The source is freed.
#
# So the arithmetic string that reaches the evaluator, and that a diagnostic
# quotes back, is a reconstruction: redirections are normalised (`>&2` becomes
# `1>&2`), comments are gone, spacing is the printer's. Everything *outside* the
# `$( … )` is still verbatim source — only the bodies are replaced.
#
# Two neighbours are deliberately left alone, and both are shown below:
#
#   * a backtick body is never parsed at this point, so it comes back exactly as
#     written — which matters, because re-printing it would drop the `\`` a
#     nested substitution needs;
#   * a `$((` whose text turns out not to be an expression falls back to a
#     command substitution (`chk_arithsub`, subst.c:10580), and it was still the
#     `P_ARITH` scan that collected the text — so its nested bodies are
#     re-printed too, even though the classification that sent the text back to
#     a command happens afterwards.
#
# Not covered here: a body whose re-print bash lays out over lines of its own
# (`if`, `for`, `while`, `until`, `select`, `case`, a function definition). osh's
# printer keeps those on one line, which shifts a `$LINENO` written after them
# inside the same body. See `known-issues.md`,
# TD-OILS-A-REPRINTED-COMPOUND-COMMAND-IS-KEPT-ON-ONE-LINE.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }
unset x

# `${x!}` is the lever throughout: it is a `bad substitution`, and bash reports
# it by quoting the whole arithmetic string back — which is how the stored text
# becomes visible without running anything.
echo "=== the string names the re-print, not what was written ==="
e 'echo $(( ${x!} + $(echo a>&2) ))'
e 'echo $(( ${x!} + $(  echo   a  ) ))'
e 'echo $(( ${x!} + $(echo a # hi
) ))'
e 'echo $(( ${x!} + $(echo "a  b" | cat) ))'
e 'echo $(( ${x!} + $(echo a; echo b) ))'
e 'echo $(( ${x!} + $( { echo a; } ) ))'
# The re-print of a subshell opens with `(`, so `parse_comsub` puts a space in
# front of it (parse.y:4221–4227) rather than let the result read as `$((`.
e 'echo $(( ${x!} + $( (echo a) ) ))'

echo "=== every spelling of an arithmetic string ==="
e 'echo $[ ${x!} + $(echo a>&2) ]'
e 'echo "$(( ${x!} + $(echo a>&2) ))"'
e 'let "${x!} + $(echo a>&2)"'
e 'echo ${a[${x!}+$(echo a>&2)]}'

echo "=== more than one, and one inside another ==="
e 'echo $(( ${x!} + $(echo a>&2) + $(echo b>&2) ))'
e 'echo $(( ${x!} + $(echo $(echo c>&2) >&2) ))'
e 'echo $(( ${y:-$(echo a>&2)} + ${x!} ))'
e 'echo $(( ${x!} + $(cat <<E
hi
E
) ))'

echo "=== a backtick is not re-parsed, so it comes back verbatim ==="
e 'echo $(( ${x!} + `echo a>&2` ))'
e 'echo $(( ${x!} + `echo   a` ))'

echo "=== and a \$(( that is not an expression keeps the same re-print ==="
f() { : $(( echo $( (echo a) ) ) ); }
declare -f f
g() { : $(( echo $(echo a>&2) ) ); }
declare -f g

echo "=== the printback shows it too ==="
h() {
  : $(( 1 + $( (echo 2) ) ))
  : $[ $(echo a>&2) ]
  : $(( ${y:-$( (echo 2) )} ))
}
declare -f h

echo "=== and the re-print is what runs ==="
echo $(( 1 + $(echo 2>/dev/null; echo 3) ))
echo $(( 1 + $( (echo 2) ) ))

echo done
