# `SECONDS`, `RANDOM`, `LINENO` and the rest are not variable-table entries:
# each is a pair of shell functions, one that computes a value on a read and one
# that takes a write. The write function is what makes `SECONDS=5` restart the
# clock at 5 rather than freeze the name at "5", and what makes `LINENO=99` do
# nothing lasting at all.
#
# The function belongs to the **name**, not to the `NAME=value` syntax, so every
# way of writing the variable reaches it: `read`, `printf -v`, a `for` loop's
# control variable, `select`, `getopts`, arithmetic, and a nameref pointing at
# one. Each of them therefore
#
#   * runs the name's own side effect — the clock rebases, the generator
#     reseeds, the subshell counter moves, `$0` changes;
#   * reads the string as a *number*, so a non-numeric one is 0;
#   * leaves no ordinary variable behind, which is why the name goes on
#     computing afterwards and keeps its own attribute letters.
#
# The binding is the global's, so anything that puts an ordinary variable of the
# same name in front of it — `local`, an assignment prefix, `unset`, or widening
# the name into an array — takes all of that away for as long as it stands.

echo '=== every write rebases the clock, and none of them stores a string'
# The clock is only ever asked about in a range, and a listing only ever for
# its attribute letters: the second it happens to land on is the shell's own
# wall clock, so an exact value would be a coin toss at every boundary.
( read SECONDS <<< 5; d=$(declare -p SECONDS); echo "read: $((SECONDS>=5)) ${d%%=*}" ) 2>&1
( printf -v SECONDS 5; d=$(declare -p SECONDS); echo "printfv: $((SECONDS>=5)) ${d%%=*}" ) 2>&1
( for SECONDS in 5; do :; done; d=$(declare -p SECONDS); echo "for: $((SECONDS>=5)) ${d%%=*}" ) 2>&1
( ((SECONDS=5)); d=$(declare -p SECONDS); echo "arith: $((SECONDS>=5)) ${d%%=*}" ) 2>&1
( SECONDS=5; d=$(declare -p SECONDS); echo "assign: $((SECONDS>=5)) ${d%%=*}" ) 2>&1

echo '=== …and reads the string as a number, so a word is zero'
( read SECONDS <<< abc; d=$(declare -p SECONDS); echo "read: $((SECONDS<3)) ${d%%=*}" ) 2>&1
( printf -v SECONDS abc; echo "printfv: $((SECONDS<3))" ) 2>&1
( SECONDS=abc; echo "assign: $((SECONDS<3))" ) 2>&1

echo '=== the integer attribute the name carries reaches the value too'
( : $SECONDS; read SECONDS <<< 3+4; echo "touched read: $((SECONDS>=7))" ) 2>&1
( : $SECONDS; printf -v SECONDS 3+4; echo "touched printfv: $((SECONDS>=7))" ) 2>&1
( : $SECONDS; for SECONDS in 3+4; do :; done; echo "touched for: $((SECONDS>=7))" ) 2>&1
( read SECONDS <<< 3+4; echo "untouched read: $((SECONDS<3))" ) 2>&1

echo '=== …and a bad expression is a diagnostic, not a failure'
( read SECONDS <<< 1/0; echo "untouched: rc=$?" ) 2>&1
( : $SECONDS; read SECONDS <<< 1/0; echo "touched: rc=$?" ) 2>&1
( : $RANDOM; printf -v RANDOM 1/0; echo "printfv: rc=$?" ) 2>&1
( RANDOM=1/0; echo "assign: rc=$?" ) 2>&1

echo '=== the generator reseeds whichever way it is written'
( read RANDOM <<< 42; a=$RANDOM; read RANDOM <<< 42; b=$RANDOM; echo "read: $((a==b))" ) 2>&1
( printf -v RANDOM 42; a=$RANDOM; printf -v RANDOM 42; b=$RANDOM; echo "printfv: $((a==b))" ) 2>&1
( for RANDOM in 42; do :; done; a=$RANDOM; for RANDOM in 42; do :; done; b=$RANDOM; echo "for: $((a==b))" ) 2>&1
( ((RANDOM=42)); a=$RANDOM; ((RANDOM=42)); b=$RANDOM; echo "arith: $((a==b))" ) 2>&1

echo '=== the subshell counter moves under every one'
( read BASH_SUBSHELL <<< 7; echo "read: $BASH_SUBSHELL" ) 2>&1
( printf -v BASH_SUBSHELL 7; echo "printfv: $BASH_SUBSHELL" ) 2>&1
( for BASH_SUBSHELL in 7; do :; done; echo "for: $BASH_SUBSHELL" ) 2>&1
( ((BASH_SUBSHELL=7)); echo "arith: $BASH_SUBSHELL" ) 2>&1
( let BASH_SUBSHELL=7; echo "let: $BASH_SUBSHELL" ) 2>&1
( : $((BASH_SUBSHELL=7)); echo "expansion: $BASH_SUBSHELL" ) 2>&1
( for ((BASH_SUBSHELL=7; 0; )); do :; done; echo "forarith: $BASH_SUBSHELL" ) 2>&1
( set -- -a; getopts a BASH_SUBSHELL; echo "getopts: $BASH_SUBSHELL rc=$?" ) 2>&1
( select BASH_SUBSHELL in a; do break; done </dev/null >/dev/null; echo "select: $BASH_SUBSHELL" ) 2>&1
( declare -n r=BASH_SUBSHELL; read r <<< 7; echo "nameref read: $BASH_SUBSHELL" ) 2>&1
( declare -n r=BASH_SUBSHELL; r=7; echo "nameref assign: $BASH_SUBSHELL" ) 2>&1

echo '=== `BASH_ARGV0` is the one whose value is a string, and it sets $0'
( read BASH_ARGV0 <<< zz; echo "read: $0" ) 2>&1
( printf -v BASH_ARGV0 zz; echo "printfv: $0" ) 2>&1
( for BASH_ARGV0 in zz; do :; done; echo "for: $0" ) 2>&1
( BASH_ARGV0=zz; echo "assign: $0" ) 2>&1

echo '=== a name whose write is discarded keeps computing after it'
( read LINENO <<< 99; echo "read: $LINENO" ) 2>&1
( printf -v LINENO 99; echo "printfv: $LINENO" ) 2>&1
( for LINENO in 99; do :; done; echo "for: $LINENO" ) 2>&1
( ((LINENO=99)); echo "arith: $LINENO" ) 2>&1
( set -- -a; getopts a LINENO; echo "getopts: $LINENO" ) 2>&1
( read EPOCHSECONDS <<< 5; d=$(declare -p EPOCHSECONDS); echo "read: $((EPOCHSECONDS>1000000)) ${d%%=*}" ) 2>&1
( read BASHPID <<< 5; echo "read: $((BASHPID>1))" ) 2>&1

echo '=== an ordinary binding in front of the name takes the function away'
( f() { local SECONDS; read SECONDS <<< abc; echo "local: $SECONDS"; }; f ) 2>&1
( f() { local BASH_SUBSHELL; read BASH_SUBSHELL <<< 7; echo "local: $BASH_SUBSHELL"; }; f ) 2>&1
( f() { read BASH_SUBSHELL <<< 7; echo "temp: $BASH_SUBSHELL"; }; BASH_SUBSHELL=9 f ) 2>&1
( unset SECONDS; read SECONDS <<< abc; echo "unset: $SECONDS" ) 2>&1
( unset RANDOM; printf -v RANDOM abc; echo "unset: $RANDOM" ) 2>&1

echo '=== …and so does widening the name into an array'
( BASH_SUBSHELL[1]=9; read BASH_SUBSHELL <<< 7; echo "elem: $(declare -p BASH_SUBSHELL)" ) 2>&1
( printf -v 'BASH_ARGV0[1]' zz; echo "sub: $0 $(declare -p BASH_ARGV0)" ) 2>&1
( LINENO[1]=9; printf -v LINENO abc; echo "elem: $(declare -p LINENO)" ) 2>&1

echo '=== a readonly one is refused before any of this'
( read PPID <<< 5; echo "read: rc=$?" ) 2>&1
( printf -v PPID 5; echo "printfv: rc=$?" ) 2>&1
( for PPID in 5; do :; done; echo "for: rc=$?" ) 2>&1
( ((PPID=5)); echo "arith: rc=$?" ) 2>&1

echo '=== the call-stack arrays are ordinary array stores, not this'
( read FUNCNAME <<< zz; echo "read: rc=$? $(declare -p FUNCNAME)" ) 2>&1
( printf -v BASH_SOURCE zz; echo "printfv: rc=$? $(declare -p BASH_SOURCE)" ) 2>&1

echo still here
