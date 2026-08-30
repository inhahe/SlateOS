# A `` ` … ` `` is two different things to `brace_gobbler`, and the difference is
# the state the word is already in.
#
# ```c
#   if (c == '\\' && (quoted == 0 || quoted == '"' || quoted == '`'))
#     { pass_next = 1; i++; continue; }
#   if (c == '$' && text[i+1] == '{' && quoted != '\'')
#     { pass_next = 1; i++; if (quoted == 0) level++; continue; }
#   if (quoted)
#     {
#       if (c == quoted) quoted = 0;
#       if (quoted == '"' && c == '$' && text[i+1] == '(') goto comsub;
#       ADVANCE_CHAR (text, tlen, i); continue;
#     }
#   if (c == '"' || c == '\'' || c == '`') { quoted = c; i++; continue; }
#   if ((c == '$' || c == '<' || c == '>') && text[i+1] == '(')
#     { comsub: si = i + 2; t = extract_command_subst (text, &si, 0); … }
#                                                     /* braces.c:637-682 */
# ```
#
# At the top level the `` ` `` row is reached, `quoted` becomes `` ` ``, and the
# body goes by unread. Inside `" … "` the `quoted` branch is taken first, so a
# backquote is only a character and the scan reads straight on into the body —
# where the `${` is treated like `\{`, a `'` is not a quote, and each `$( … )` is
# handed `extract_command_subst` over the **word's** string. What it quotes is
# therefore the rest of the word: the body's remainder, the closing backquote,
# and everything after it.
#
# Two rows do not fire in that state. `<(`/`>(` belong to the *unquoted* comsub
# row only, and a `\` still passes the next character over.
#
# Verified against bash 5.2.37.

unset z

echo '=== the top level: the backquote is a quote, and hides the body'
echo p`echo $(fi)`q{,}
echo "1 rc=$?"

echo '=== inside double quotes it is not, and the read takes the rest of the word'
echo "p`echo $(fi)`q{,}"
echo "2 rc=$?"
echo "p`echo ${z:-'$(fi)'}`q{,}"
echo "3 rc=$?"
echo "p`echo '$(fi)'`q{,}"
echo "4 rc=$?"
echo p"`echo $(fi)`"q{,}
echo "5 rc=$?"
echo "p${z:-`echo $(fi)`}q{,}"
echo "6 rc=$?"

echo '=== the `$(` row is the only one that fires there'
# `<(` is the unquoted row's alone, so this one is left for the backquote to run.
echo "p`echo <(fi)`q{,}"
echo "7 rc=$?"
# A `\` passes the next character over, in the `"` state and in the `` ` `` one.
echo "p`echo \$(fi)`q{,}"
echo "8 rc=$?"
echo "p`echo ${z:-\$(fi)}`q{,}"
echo "9 rc=$?"
# A backquote nested in the body is a quote again — `quoted` is 0 by then.
echo "p`echo \`fi\``q{,}"
echo "10 rc=$?"

echo '=== a `$((`/`$[` in the body, read by the same two rows'
echo "p`echo $((1+$(fi)))`q{,}"
echo "11 rc=$?"
echo "p`echo $[1+$(fi)]`q{,}"
echo "12 rc=$?"

echo '=== no `{` in the word, so the scanner never runs at all'
echo "p`echo $(fi)`q"
echo "13 rc=$?"

echo '=== a body that parses is read and thrown away, not run'
echo "p`echo $(echo SIDE)`q{,}"
echo "14 rc=$?"

echo '=== the failure is the scanner’s, so it drops the one command'
v=1
echo "p`echo $(fi)`q{,}"
echo "15 rc=$? v=$v"
echo TAIL
