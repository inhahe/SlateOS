# The companion to
# a-pending-here-document-is-gathered-before-the-token-that-ends-the-list-is-blamed.sh.
# There the reduction in `simple_list`'s yacc action (parse.y:1217) gathered a
# body that *was* there and moved the blame down by its lines. Here there is no
# body at all: an unclosed construct written after the `<<` swallows the rest of
# the input, so the newline that would have triggered the ordinary gather never
# arrives — and the same reduction still runs, still calls
# `gather_here_documents()`, and `make_here_document` still raises
#
#     warning: here-document at line N delimited by end-of-file (wanted `E')
#
# It reaches the reduction because yacc performs its default reductions on the
# way to discovering that the lookahead is an error; the chain from
# `simple_command` up to `simple_list` is all default reductions, so the action
# fires before the token is looked at closely enough to be rejected. Two
# consequences, both visible below:
#
#   * the warning is printed **after** the syntax error, because the unclosed
#     construct was reported by the *lexer*, before yacc ever reduced. (Contrast
#     the ordinary newline gather, whose warning comes out first — `cat <<E` at
#     the end of a script warns and then runs `cat`.)
#   * both of its line numbers are the end of the input. The reader ran all the
#     way there looking for the construct's close, so `line_number` was already
#     at the bottom when the reduction finally gathered — the number does not
#     depend on where the `<<` was written, only on how many lines the input has.
#
# Which constructs count is not "unterminated anything". Everything bash reads
# with `parse_matched_pair` — `"`, `'`, a backtick, `${`, `$((` — leaves
# `need_here_doc` alone and so warns. `$( … )` does not: `parse_comsub`
# (parse.y:4133) zeroes `need_here_doc` before running its nested `yyparse`, and
# the `EOF_Reached` path returns before `restore_parser_state` can put the saved
# copy back, so the pending here-documents are simply forgotten. Process
# substitution goes through `parse_comsub` too and forgets them as well. `$((`
# looks like a counterexample and is not: `parse_comsub` checks for the second
# `(` and returns into `parse_matched_pair` with `P_ARITH` (parse.y:4103) above
# the zeroing — so `$((` keeps them however it is finally read, backtracking to
# a substitution or not.
#
# And, as with the other file, it is the *top-level* list only: `compound_list`'s
# copy of the action (parse.y:1147) is guarded on the previous token having been
# a newline, so nothing inside a `{ }`, an `if`, a `( )`, a loop, a `case` arm or
# a function body reduces, gathers, or warns.
#
# Each probe runs under `eval` in a subshell so one syntax error does not abandon
# the rest of the input, which also puts the line numbers on a scale of their own
# starting at the first line of the fragment. Note the diagnostic and the warning
# do not agree on their prefix: the error is `parser_error`'s and carries the
# `eval:` input-source token, the warning is a plain `internal_warning` and does
# not.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the warning comes below the error, at the end of the input"
e 'cat <<E "'

echo "=== …and the number is the input's length, not the operator's line"
e 'cat <<E "
body
E'
e 'echo one
cat <<E "'

echo "=== every parse_matched_pair construct keeps the pending record"
e "cat <<E '"
e 'cat <<E `'
e 'cat <<E ${'
e 'cat <<E $(('
e 'cat <<E $(( 1 +'
# The arithmetic read fails and the text is re-read as a substitution — but the
# zeroing sits below the `$((` check, so this one still warns.
e 'cat <<E $(( echo a )'

echo "=== a command substitution forgets them instead"
e 'cat <<E $('
e 'cat <<E <('
e 'cat <<E >('
# Nesting does not matter: whichever construct the input dies in is the one that
# decides, and a `$(` inside a `"` is still a `$(`.
e 'cat <<E "$('
e 'cat <<E $("'

echo "=== two pending are gathered in declaration order, at the same line"
e 'cat <<A <<B "'

echo "=== one already gathered at a newline is no longer pending"
e 'cat <<A
a
A
cat <<B "'

echo "=== <<- and a quoted delimiter change nothing"
e 'cat <<-E "'
e 'cat <<"E" "'

echo "=== a longer input moves the line, an unread body does not"
e 'cat <<E "
b1
b2
b3'

echo "=== nothing reduces inside a compound, so nothing warns"
e '{ cat <<E "'
e 'if true; then
cat <<E "'
e 'while cat <<E "'
e 'until cat <<E "'
e 'for x in a; do cat <<E "'
e 'case a in a) cat <<E "'
e 'f() { cat <<E "'
e '( cat <<E "'

echo "=== a compound that already closed leaves the list at the top level again"
e '{ echo x; }
cat <<E "'
e 'if true; then echo x; fi
cat <<E "'

echo "=== a pipeline and a && list reduce the same way"
e 'echo x | cat <<E "'
e 'echo x && cat <<E "'
e 'echo x; cat <<E "'
