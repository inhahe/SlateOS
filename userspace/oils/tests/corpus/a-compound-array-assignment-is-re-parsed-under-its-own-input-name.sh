# A `$( … )` is read by the parser twice — once where it is written, once over
# its own re-print at expansion time — and a re-print that will not parse back is
# blamed on `command substitution`. That much is
# a-cmdsub-body-is-re-read-as-written-not-as-reprinted.sh.
#
# A **compound array assignment** never reaches that second read. bash does not
# keep its elements apart at all: `parse_compound_assignment` (parse.y:4715)
# reads them with `read_token` under `PST_COMPASSIGN` and writes them back out
# as one string joined by a *single space* — which is why every diagnostic below
# echoes `one two "p$(! )q" four` for a literal written across four lines. Then
# `assign_compound_array_list` re-reads that whole string before expanding any
# of it:
#
#   list = parse_string_to_word_list (val, 1, "array assign");   (arrayfunc.c:587)
#
# The string already holds each substitution's re-print, so a `$( … )` in it is
# met by *this* read's tokenizer, and the failure is `parse_comsub`'s own
# `jump_to_top_level (FORCE_EOF)` (parse.y:4185). Four things follow:
#
#   * the input name is `array assign`;
#   * the line is counted inside the listing, from 1 — `parse_string_to_word_list`
#     does `push_stream (1)`, the argument that resets `line_number`, where the
#     `push_stream (0)` of `parse_string` leaves the ordinary re-parse's counter
#     running on from the executing command;
#   * the echoed line is that listing's own line, whole, because the reader is
#     standing in the string and not in the script;
#   * and it is fatal, where a scalar `v="p$(⏎!⏎)q"` only discards its own unit.
#
# A backtick body is not re-read (the tokenizer takes it as a matched pair
# without parsing), and neither is a compound assignment used as a *command
# prefix* — that one is expanded as an ordinary word. Both are pinned below.
#
# Every case runs in a subshell, because the abort would otherwise take the rest
# of the file with it.
q() { ( eval "$1" ) 2>&1; echo "rc=$?"; }

echo "=== the name is array assign, and the line is 1 wherever it is written"
q 'a=( "p$(
!
)q" r ); echo no'
q 'echo one; echo two; a=( "p$(
!
)q" r ); echo no'
q 'declare -a b=( "p$(
!
)q" ); echo no'
q 'a=( x ); a+=( "p$(
!
)q" ); echo no'
q 'declare -A m=( [k]="p$(
!
)q" ); echo no'
q 'f() { local a=( "p$(
!
)q" ); }; f; echo no'
q 'export a=( "p$(
!
)q" ); echo no'
q 'readonly a=( "p$(
!
)q" ); echo no'

echo "=== the echoed line is the listing, joined with single spaces"
q 'a=( one
     two
     "p$(
!
)q"
     four ); echo no'
q 'a=( [0]=$(
!
) [2]=z ); echo no'
q 'a=( [$(
!
)]=x ); echo no'
q 'a=( "$(
!
)" "$(
time
)" ); echo no'
q 'a=( "$( echo "x$(
!
)y" )" ); echo no'

echo "=== a newline the listing kept moves both the number and the echo"
q 'a=( "x
y" "p$(
!
)q" ); echo no'
q 'a=( "x
y$(
!
)z" ); echo no'
q 'a=( "$(cat <<E
hi
E
)" "p$(
!
)q" ); echo no'

echo "=== it is fatal, and takes the rest of the input with it"
q 'a=( "p$(
!
)q" ); echo no-1; echo no-2'
q '( a=( "p$(
!
)q" ) ); echo yes-outer'
q 'eval '\''a=( "p$(
!
)q" )'\''; echo no'

echo "=== and only where the re-parse happens"
# A readonly name and a reference designating an element are both refused before
# `assign_compound_array_list` is reached.
q 'declare -r ro=1; ro=( "p$(
!
)q" ); echo yes'
q 'declare -n r="base[1]"; r=( "p$(
!
)q" ); echo yes'
# A backtick body is a matched pair to the tokenizer, never a parse.
q 'a=( "p`
!
`q" ); echo "yes ${a[0]}"'
# A prefix assignment is expanded as an ordinary word, so it is the *command
# substitution* that answers — and it is not fatal.
q 'a=( "p$(
!
)q" ) echo hi; echo yes'
# Words that only look like keywords are words here.
q 'a=( if then esac ); echo "${a[@]}"'
q 'a=( "p$(echo z)q" r ); echo "${a[@]}"'
echo "=== done"
