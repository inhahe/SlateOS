# `]]` is a reserved word, and not only inside `[[ … ]]`. bash's
# `word_token_alist` lists it beside `fi`, `done` and `}`:
#
#   { "[[", COND_START },
#   { "]]", COND_END },
#
# and `CHECK_FOR_RESERVED_WORD` consults that table on the single condition
# that a reserved word is *acceptable here* — `reserved_word_acceptable
# (last_read_token)`, which is to say the parser is at the start of a command.
# Nothing in the macro asks whether a `[[` is open. The one line that mentions
# the conditional state only clears it:
#
#   else if (word_token_alist[i].token == COND_END) \
#     parser_state &= ~(PST_CONDCMD|PST_CONDEXPR); \
#
# So a `]]` written where a command could start is the token `COND_END`, the
# grammar has no production that begins with it, and the parse dies —
# `syntax error near unexpected token `]]'`. It is the same reason `fi` alone
# is an error, not a command named `fi`.
#
# The other half of the rule is that `reserved_word_acceptable` is false
# everywhere else, so `]]` past the command word is an ordinary word with
# nothing special about it at all. Both halves are below, because only the pair
# of them says what the rule is: it is about *position*, not about spelling.
#
# Each probe runs under `eval` in a subshell — a syntax error otherwise
# abandons the rest of the input.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== where a command could start, it is a token and the parse dies"
e ']]'
e 'true; ]]'
e ']] ]]'
e 'true && ]]'
e 'true || ]]'
e 'true | ]]'
e '{ ]]; }'
e '( ]] )'
e 'if ]]; then :; fi'
e 'while ]]; do :; done'
# The `until` body breaks at once on purpose: a shell that mistakes `]]` for a
# command finds the condition failing forever, and this case would not end.
e 'until ]]; do break; done'
e 'for x in a; do ]]; done'
e 'case a in a) ]];; esac'

echo "=== past the command word it is a word like any other"
e 'echo ]]'
e 'echo a ]] b'
e 'x=]]; echo $x'
e 'for x in ]]; do echo $x; done'
e 'case ]] in ]]) echo hit;; esac'
e 'echo "]]" ${x-]]}'

echo "=== quoting takes the token away, as it does from every reserved word"
# `CHECK_FOR_RESERVED_WORD` refuses outright when the word was quoted or held a
# `$` — `if (!dollar_present && !quoted && …)` — so these are commands whose
# name happens to be `]]`.
e '"]]"'
e '\]]'
e "']]'"
e 'q=]]; $q'

echo "=== and the pairing it exists for still closes"
e '[[ x ]]; echo $?'
e '[[ x ]] ]]'
e '[[ ]]'
