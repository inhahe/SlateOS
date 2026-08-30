# bash deparses a command in exactly one place — `make_command_string_internal`
# (print_cmd.c:182-378) — and the three callers differ only in which flag they
# set first:
#
#   print_function_def / named_function_string   inside_function_def   `declare -f`
#   print_comsub                                 printing_comsub       `$( … )` re-print
#   make_command_string                          neither               what `jobs` shows
#
# Layout is otherwise one global, `indentation`, stepped by
# `indentation_amount` (4). So a compound command inside a substitution is laid
# out over lines just as it is at the top level — `$(if true; then echo a; fi)`
# comes back as three lines, not one.
#
# The re-print starts at **column 0** even when the substitution is nested,
# because `print_comsub` runs at *parse* time, while `indentation` is still 0.
# Nothing has raised it yet: `print_function_def` raises it only when the stored
# function is later printed, long after the body text was fixed.
#
# Two details this pins that are easy to get wrong by hand:
#
#   * `semicolon()` (print_cmd.c:1512-1521) is conditional — it writes `;`
#     *unless* the last byte printed was `&` or `\n`. So a backgrounded last
#     statement gets no `;`, and a here-document body (which ends in its own
#     newline) is followed by a blank line instead.
#   * `print_case_clauses` joins patterns with
#     `command_print_word_list (clauses->patterns, " | ")` (print_cmd.c:769) —
#     `a|b)` in the source comes back as `a | b)`.
#
# And one grammar fact that makes the `\n` connector unreachable outside a
# substitution: `list1`'s `list1 '\n' newline_list list1` rule records `'\n'` as
# the connector only while `parser_state & PST_CMDSUBST`, and `';'` otherwise.
# That is why `{ echo a` + newline + `echo b; }` in a function body prints with
# `; ` while the same text inside `$( … )` keeps its line break.

echo "=== a compound command inside a substitution is laid out over lines"
c1() { : $(if true; then echo a; fi); }
c2() { : $(for i in a b; do echo $i; done); }
c3() { : $(while false; do echo a; done); }
c4() { : $(case x in a|b) echo y ;; esac); }
c5() { : $(f() { echo a; }); }
declare -f c1 c2 c3 c4 c5

echo "=== nested substitutions still re-print from column 0"
n1() { : $(echo $(if true; then echo a; fi)); }
declare -f n1

echo "=== the newline connector survives only inside a substitution"
s1() { : $(echo a
echo b); }
s2() {
  echo a
  echo b
}
declare -f s1 s2

echo "=== semicolon() skips a statement already ended by & or a heredoc"
h1() {
  sleep 0 &
  cat <<EOF
body
EOF
  echo z
}
declare -f h1

# `named_function_string` — the form `export -f` puts in the environment — is the
# same printer again, with `indentation = 1` and `indentation_amount = 0` saved
# over the call, so every body line carries exactly one leading space no matter
# how deep it nests, and the restored `indentation` (0) puts `}` at column 0.
echo "=== the exported form is the same printer at indentation 1, step 0"
e1() { if true; then echo a; fi; for i; do echo $i; done; }
export -f e1
declare -f e1
env | awk '/^BASH_FUNC/,/^}$/'

echo "=== a for/select head always prints its word list"
w1() { for i; do :; done; }
w2() { for i in; do :; done; }
w3() { select i; do :; done; }
declare -f w1 w2 w3

echo "=== a subshell body keeps the subshell's own depth"
b1() { ( echo a; echo b ); }
b2() { { echo a; echo b; }; }
declare -f b1 b2
