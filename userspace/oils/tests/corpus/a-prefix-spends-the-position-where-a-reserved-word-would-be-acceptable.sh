# bash recognises a reserved word only where one would be *acceptable*, and that
# is a property of the token before it, not of the command being empty.
# `CHECK_FOR_RESERVED_WORD` (parse.y:2994) looks the word up in
# `word_token_alist` under the single condition `reserved_word_acceptable
# (last_read_token)`, and that function's list (parse.y:5367) is separators,
# operators, and other reserved words — `\n ; ( ) | & { } && || ;; ;& ;;& |& !
# ]] do done elif else esac fi if then time coproc until while` — plus 0, for
# the very start of the input.
#
# A plain `WORD` is not on the list. An assignment prefix is a plain `WORD`, and
# so is the filename of a redirection, so after either one every reserved word
# is an ordinary command name and bash goes looking for it on `$PATH`.
#
# Note which openers are *absent* from the list: `for`, `case`, `select`,
# `function`. That is why `for done in a` names a variable `done`, why
# `case done in done)` matches, and why `function done { …; }` defines a
# function.
#
# The words have to be written out rather than expanded — a reserved word is
# recognised when it is *lexed*, so `v=1 $w` never produces one whatever `$w`
# holds. And a syntax error ends the unit it was read from, so the probes that
# are meant to fail live in their own files, run with `.` so that failing does
# not end this script.
#
# Verified against bash 5.2.37.

mk() { printf '%s\n' "$2" > "$1"; }

echo "=== after an assignment prefix every reserved word is a command name"
v=1 done
v=1 then
v=1 fi
v=1 do
v=1 esac
v=1 in
v=1 }
v=1 !
v=1 {
v=1 ]]
v=1 elif
v=1 else
v=1 while
v=1 until
v=1 if
v=1 for
v=1 case
v=1 select
echo "rc=$?"

echo "=== and after a redirection's filename too"
>/dev/null done
>/dev/null then
>/dev/null }
echo "rc=$?"

echo "=== the assignment still happens, and only for that command"
v=1 echo hi
echo "v=[${v-unset}]"
a=1 b=2 env >/dev/null
echo "rc=$?"

echo "=== the compound openers come along, so the error moves to the next acceptable slot"
mk e1 'v=1 if true; then echo y; fi'
. ./e1
mk e2 'v=1 while true; do :; done'
. ./e2
mk e3 'v=1 function f { :; }'
. ./e3
mk e4 'v=1 until false; do :; done'
. ./e4

echo "=== with nothing after them they simply run"
v=1 while true
v=1 if true
v=1 function f
echo "rc=$?"

echo "=== the positions where one IS acceptable are unchanged"
mk f1 'echo a; done'
. ./f1
mk f2 '! done'
. ./f2
mk f3 'time done'
. ./f3
mk f4 '{ echo a; } done'
. ./f4
mk f5 '(echo a) done'
. ./f5
mk f6 'echo a | done'
. ./f6
mk f7 'for i in 1; do done; done'
. ./f7
mk f8 'coproc done { echo y; }'
. ./f8

echo "=== and a prefix inside a loop body does not swallow the body's own closer"
for i in 1 2; do v=$i done; done
echo "rc=$?"

echo "=== openers absent from the list keep their operand an ordinary word"
for done in a b; do echo "loop $done"; done
case done in done) echo "case hit";; esac
function done { echo "fn done"; }
type -t done
# Calling it needs the quotes: `CHECK_FOR_RESERVED_WORD` is gated on `!quoted`
# as well, so a bare `done` here would be the reserved word and a syntax error.
"done"
