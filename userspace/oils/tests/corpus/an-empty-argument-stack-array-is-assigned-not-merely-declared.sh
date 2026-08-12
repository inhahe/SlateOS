# bash *assigns* `BASH_ARGC` and `BASH_ARGV` rather than merely declaring them,
# so an empty one prints `declare -a BASH_ARGV=()` and not the bare
# `declare -a BASH_ARGV` a declaration leaves behind. That distinction is
# `invisible_p`'s, and it is the same one `declare -a q` and `q=()` draw.
#
# It is reachable because the two stacks empty *independently*: a frame for a
# call with no arguments puts a count on `BASH_ARGC` and nothing at all on
# `BASH_ARGV`, so the pair can be a non-empty array beside an empty one.
#
# Everything here turns `extdebug` on first. Without it osh populates neither
# — a deliberate, documented divergence: bash exposes an undocumented base
# frame whose observable behaviour contradicts its own man page ("The shell
# sets BASH_ARGC only when in extended debugging mode"). See known-issues.md
# TD-OILS-MISSING-SPECIAL-ARRAYS.

shopt -s extdebug

echo "--- a call with no arguments leaves BASH_ARGV empty but assigned"
t1() { f() { declare -p BASH_ARGV; }; f; }; t1
t2() { f() { declare -p BASH_ARGC; }; f; }; t2
t3() { f() { echo "n=${#BASH_ARGV[@]} c=${#BASH_ARGC[@]}"; }; f; }; t3
t4() { f() { echo "[${BASH_ARGV[@]:-U}]"; }; f; }; t4

echo "--- …and with arguments it fills"
t5() { f() { declare -p BASH_ARGV BASH_ARGC; }; f a b; }; t5
t6() { f() { g() { declare -p BASH_ARGV BASH_ARGC; }; g; }; f a b; }; t6

echo "--- the frame outlives the call and the shopt"
t7() { f() { :; }; f; shopt -u extdebug; declare -p BASH_ARGV; shopt -s extdebug; }; t7

echo "--- the unfiltered listing spells it the same way"
t8() { f() { declare -p | grep -E '^declare -a BASH_ARGV'; }; f; }; t8
t9() { f() { declare -p | grep -E '^declare -a BASH_ARGC'; }; f a; }; t9

echo "--- and the questions an assigned-but-empty array answers"
ta() { f() { [[ -v BASH_ARGV ]]; echo "v=$?"; }; f; }; ta
tb() { f() { [[ -v BASH_ARGV[0] ]]; echo "v0=$?"; }; f; }; tb
tc() { f() { set -u; echo "n=${#BASH_ARGV[@]}"; }; f; }; tc
td() { f() { echo "[${BASH_ARGV[*]}]"; echo "st=$?"; }; f; }; td
