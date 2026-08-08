# bash's `function_body` is `shell_command`, not a brace group.
#
# The production is `function_body: shell_command | shell_command
# redirection_list` (parse.y), and `shell_command` is *every* compound command:
# `for`, the arithmetic `for`, `case`, `while`, `until`, `select`, `if`, a
# subshell, a group, an arithmetic command and a conditional. All eleven define
# a function; there is no separate rule for the two that get written most.
#
# `declare -f` is the discriminator, because it re-prints from the parse tree
# and so shows which node the body actually became. Two things it settles:
#
# The brace group is the arm that is *not* wrapped — `make_function_def` takes
# the group's command directly, so one pair of braces comes back, not two. Every
# other arm keeps its own node inside the braces `declare -f` always prints.
#
# And the redirection list goes with the *body*, except after a brace group.
# `f() { :; } >/dev/null` prints `} > /dev/null`, outside; `f() ( : ) >/dev/null`
# prints `( : ) > /dev/null`, inside. Same list, same position in the source,
# two different owners — which is only visible through `declare -f`.
#
# The set is no wider than `shell_command`: a nested definition, a negation and
# a simple command are all still syntax errors, so they live in their own files
# run with `.`.
#
# Verified against bash 5.2.37.

mk() { printf '%s\n' "$2" > "$1"; }

echo "=== every compound command is a function body"
a1() if true; then echo hi; fi
declare -f a1; a1
a2() for x in a b; do echo "x=$x"; done
declare -f a2; a2
a3() while false; do :; done
declare -f a3; a3; echo "a3 rc=$?"
a4() until true; do :; done
declare -f a4; a4; echo "a4 rc=$?"
a5() case x in x) echo m;; esac
declare -f a5; a5
a6() [[ a ]]
declare -f a6; a6; echo "a6 rc=$?"
a7() ((1))
declare -f a7; a7; echo "a7 rc=$?"
a8() for ((i=0;i<2;i++)); do echo "i=$i"; done
declare -f a8; a8
a9() { echo brace; }
declare -f a9; a9
a10() ( echo sub )
declare -f a10; a10

echo "=== the function keyword form takes the same bodies"
function b1 if true; then echo hi; fi
declare -f b1; b1
function b2 () ((1))
declare -f b2; b2; echo "b2 rc=$?"
function b3 case x in x) echo m;; esac
declare -f b3; b3

echo "=== a redirection after a brace group belongs to the function"
c1() { echo out; } >/dev/null
declare -f c1; c1; echo "c1 rc=$?"

echo "=== after anything else it belongs to the body"
c2() ( echo out ) >/dev/null
declare -f c2; c2; echo "c2 rc=$?"
c3() ((1)) >/dev/null
declare -f c3; c3; echo "c3 rc=$?"
c4() if true; then echo out; fi >/dev/null
declare -f c4; c4; echo "c4 rc=$?"
c5() for x in a; do echo "$x"; done >/dev/null
declare -f c5; c5; echo "c5 rc=$?"

echo "=== a body's scope is its own, so a subshell still cannot leak"
d1() ( cd /; dx=1 )
d1; echo "dx=${dx-unset}"

echo "=== and the set is no wider than shell_command"
mk e1 'f() f2() { :; }'
. ./e1
mk e2 'f() ! true'
. ./e2
mk e3 'f() echo hi'
. ./e3
mk e4 'f() x=1'
. ./e4
mk e5 'f() then'
. ./e5
mk e6 'function f g ((1))'
. ./e6
