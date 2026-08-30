# `(( … ))` is read by the same scan the `$(( … ))` expansion is read by:
# `parse_arith_cmd` calls `parse_matched_pair (0, '(', ')', &ttoklen, P_ARITH)`
# (parse.y:4519–4530). So everything the sibling case
# `an-arithmetic-string-carries-its-command-substitution-re-printed.sh` shows
# about the *expansion* holds for the *command* too: a `$( … )` inside it is
# parsed as the scan steps over it, and what the scan keeps is
# `print_comsub`'s re-print, not the source (parse.y:4219–4241).
#
# Two consequences that only the command form can show:
#
#   * the body is parsed *before* the adjacency test. `((echo $(fi) ) )` is not
#     `( (echo $(fi) ) )` with a bad substitution inside — it is a fatal syntax
#     error at `fi`, because the `P_ARITH` scan reached the `$(fi)` and parsed
#     it, and only afterwards would the missing `))` have sent the text back to
#     be re-read as nested subshells;
#   * a `for (( … ))` header is one such scan whose text is *then* split on `;`,
#     so each of the three sections carries re-printed bodies.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }
unset x

# `${x!}` is the lever: a `bad substitution` that makes bash quote the whole
# arithmetic string back, which is how the stored text becomes visible.
echo "=== the command keeps the re-print, not the source ==="
e '(( ${x!} + $(echo a>&2) ))'
e '(( ${x!} + $(  echo   a  ) ))'
e '(( ${x!} + $( (echo a) ) ))'
e '(( ${x!} + $(echo a>&2) + $(echo b>&2) ))'
e '(( ${y:-$(echo a>&2)} + ${x!} ))'
e '(( ${x!} + "$(echo a>&2)" ))'
e '(( ${x!} + $(cat <<E
hi
E
) ))'

echo "=== a backtick is not re-parsed here either ==="
e '(( ${x!} + `echo a>&2` ))'

echo "=== each section of a for header is the same scan ==="
e 'for (( ${x!} + $(echo a>&2) ;; )); do :; done'
e 'for (( ; ${x!} + $(echo a>&2) ; )); do :; done'
e 'for (( ;; ${x!} + $(echo a>&2) )); do :; done'

echo "=== the body is parsed before the adjacency test ==="
e '((echo $(fi) ) )'
e '(( 1 + $(fi) ) )'
e 'if false; then (( 1 + $(fi) )); fi'

echo "=== a body that parses, with the second ) not adjacent ==="
e '((echo $(echo hi) ) )'

echo "=== and the re-print is what runs ==="
e '(( $(echo 1) + 1 )); echo rc=$?'
e 'x=0; (( x = 1 + $( (echo 2) ) )); echo x=$x'

echo done
