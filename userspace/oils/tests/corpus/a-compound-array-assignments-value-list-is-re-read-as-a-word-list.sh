# A compound array assignment's value list is read twice, and the second read is
# not the parser's. `parse_compound_assignment` (parse.y) collects the words at
# parse time and joins them back into one string with single spaces; the
# assignment then carries that string, and `assign_compound_array_list` splits
# it again at *execution* time with
#
#     parse_string_to_word_list (val, 1, "array assign");   /* arrayfunc.c:587 */
#
# That reader consults no grammar at all — it loops on `read_token` and takes
# whatever comes back a `WORD` (parse.y:6398). So a re-joined list can only fail
# two ways, and bash tells them apart by *who* raises the failure:
#
#   * the listing's own tokenizer, when a construct in it is left unclosed.
#     `parse_string_to_word_list` answers that with
#
#         set_exit_status (EXECUTION_FAILURE);
#         …
#         jump_to_top_level (DISCARD);            /* parse.y:6472-6480 */
#
#     — one parse unit abandoned, `$?` at 1, the reader carries on, and an
#     enclosing `eval` contains it.
#
#   * a `$( … )` in the listing whose re-print will not parse back, which is
#     `parse_comsub`'s own `jump_to_top_level (FORCE_EOF)` (parse.y:4185) — the
#     shell ends, and `eval` does not contain it. Which
#     re-prints fail, and why, is
#     `a-cmdsub-body-is-parsed-twice-and-the-second-parse-reads-the-reprint.sh`.
#
# Either way the diagnostic is prefixed `array assign:` — the name
# `parse_string_to_word_list` pushed as the input source — and numbered from
# line 1, because its `push_stream (1)` renumbers, unlike the `push_stream (0)`
# a command substitution's re-read does. The listing is *one* line however many
# the script spread it over, so `line 1` is the only line there is; and when the
# offending line is echoed, what is echoed is the re-joined listing, not the
# script.
#
# The unclosed-construct half is reachable because a NUL cut can leave a word
# short — see `a-nul-in-a-bare-spliced-translation-cuts-the-word.sh`.
#
# Verified against bash 5.2.37.

unset x

echo "=== a re-print that will not parse back names a token in the listing"
( a=(one two "p$(
!
)q" four); echo "not reached" )
echo "subshell rc=$?"

echo "=== the listing is one line, and it is the listing that is echoed"
( a=(one $(
time
) two); echo "not reached" )
echo "subshell rc=$?"

echo "=== eval does not contain that one: it is raised below eval's guard"
f() { eval 'a=(x "$(
!
)" y); echo "not reached"'; echo "not reached either"; }
( f; echo "not reached" )
echo "subshell rc=$?"

echo "=== an unclosed construct is the tokenizer's instead, and only discards"
a=("${x:-$'a\0b'}" tail)
echo "after rc=$?"
echo "n=${#a[@]}"

echo "=== which eval does contain"
eval 'a=("${x:-$'"'"'a\0b'"'"'}" tail); echo "not reached"'
echo "after eval rc=$?"

echo "=== and which leaves the array as it was, not half assigned"
b=(keep me)
b=(one "${x:-$'a\0b'}" three)
echo "after rc=$?"
echo "b=[${b[*]}] n=${#b[@]}"

echo "=== errexit takes the parser's own exit, before the class is decided"
# `parser_error` ends `if (exit_immediately_on_error) exit_shell
# (last_command_exit_value = 2)` (error.c:386) — reached from the message, so
# the discard never happens and the status is 2, not 1.
( set -e; a=("${x:-$'a\0b'}" tail); echo "not reached" )
echo "errexit rc=$?"

echo "=== a listing that re-reads cleanly is silent, however it was written"
c=(one "two three" $'four\tfive' "$(echo six)")
printf '[%s]' "${c[@]}"; echo " n=${#c[@]}"
# The join is by single spaces, so the words' own spacing is not preserved —
# but they are re-split on the same quoting they were written with.
d=(a    b        "c  d")
printf '[%s]' "${d[@]}"; echo " n=${#d[@]}"

echo done
