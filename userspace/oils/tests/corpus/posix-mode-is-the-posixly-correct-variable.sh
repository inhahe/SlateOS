# `set -o posix` has no state of its own. bash keeps posix mode in
# `$POSIXLY_CORRECT`, and the option and the variable are the *same* switch seen
# from two sides:
#
#   * giving the name any value at all — including the empty string, including
#     through a `local` or an assignment prefix — turns the option on, and
#     taking the value away turns it off. The option is therefore scoped like a
#     variable: a `local POSIXLY_CORRECT=1` is posix mode for the call and no
#     longer.
#   * `set -o posix` writes `y`, but only when the name has no value yet, so a
#     value the script chose is left as it is. `set +o posix` unsets the name —
#     reaching past `readonly`, since it is the shell unbinding the variable
#     rather than the `unset` builtin refusing to.
#   * the state shows up wherever an option does: `set -o`, `set +o`,
#     `$SHELLOPTS`, `[[ -o posix ]]`, `[ -o posix ]` and `shopt -o posix`. It
#     is *not* one of the `$-` letters, which posix mode has never had.
#
# The one posix-mode behaviour pinned here is the one that makes the switch
# self-referential: POSIX says an assignment prefix on a **special builtin** is
# an ordinary assignment, so it outlives the command — and a
# `POSIXLY_CORRECT=1 eval :` is the prefix that turns the rule on, so it keeps
# itself. The prefix lands through the ordinary assignment path, which is why a
# `-i` on the name it displaced applies to the value the prefix carried.

st() {
  [[ -o posix ]] && echo "  [[ ]] on" || echo "  [[ ]] off"
  [ -o posix ] && echo "  [ ] on" || echo "  [ ] off"
  echo "  SHELLOPTS=[$SHELLOPTS]"
}

echo "=== off by default"
set -o | grep '^posix'
st
echo "  dash=[$-]"

echo "=== set -o posix turns it on and names the variable"
set -o posix
st
declare -p POSIXLY_CORRECT
echo "  dash=[$-]"
set -o | grep '^posix'
set +o | grep 'posix'

echo "=== set +o posix takes the variable away"
set +o posix
st
echo "  [${POSIXLY_CORRECT-unset}]"

echo "=== every shape of assignment turns it on"
for v in '' 0 1 n no y; do
  eval "POSIXLY_CORRECT=\$v"
  [[ -o posix ]] && echo "  [$v] on" || echo "  [$v] off"
  unset POSIXLY_CORRECT
done
export POSIXLY_CORRECT=1; [[ -o posix ]] && echo "  export on" || echo "  export off"
unset POSIXLY_CORRECT
declare POSIXLY_CORRECT=1; [[ -o posix ]] && echo "  declare on" || echo "  declare off"
unset POSIXLY_CORRECT
[[ -o posix ]] && echo "  unset on" || echo "  unset off"

echo "=== a plain assignment reaches SHELLOPTS too"
POSIXLY_CORRECT=1
echo "  [$SHELLOPTS]"
declare -p SHELLOPTS
unset POSIXLY_CORRECT
echo "  [$SHELLOPTS]"

echo "=== set -o posix leaves an existing value alone"
POSIXLY_CORRECT=x
set -o posix
echo "  [$POSIXLY_CORRECT]"
set -o posix
echo "  [$POSIXLY_CORRECT]"
set +o posix
echo "  [${POSIXLY_CORRECT-unset}]"
set -o posix
echo "  [${POSIXLY_CORRECT-unset}]"
set +o posix

echo "=== set +o posix reaches past readonly"
readonly POSIXLY_CORRECT=1
[[ -o posix ]] && echo "  on" || echo "  off"
set +o posix
echo "  rc=$?"
[[ -o posix ]] && echo "  on" || echo "  off"
POSIXLY_CORRECT=again
echo "  [${POSIXLY_CORRECT-unset}]"
unset POSIXLY_CORRECT

echo "=== the option is scoped like the variable"
inner() {
  [[ -o posix ]] && echo "  inner on" || echo "  inner off"
  echo "  inner SHELLOPTS=[$SHELLOPTS]"
}
outer() { local POSIXLY_CORRECT=z; inner; }
outer
[[ -o posix ]] && echo "  after on" || echo "  after off"
echo "  after SHELLOPTS=[$SHELLOPTS]"

valueless() { local POSIXLY_CORRECT; [[ -o posix ]] && echo "  local on" || echo "  local off"; }
valueless
[[ -o posix ]] && echo "  after on" || echo "  after off"

echo "=== an assignment prefix is a scope too"
pfx() { [[ -o posix ]] && echo "  pfx on" || echo "  pfx off"; }
POSIXLY_CORRECT=1 pfx
[[ -o posix ]] && echo "  after on" || echo "  after off"

echo "=== set -o posix inside a function is not local to it"
setter() { set -o posix; }
setter
[[ -o posix ]] && echo "  after on" || echo "  after off"
set +o posix

echo "=== a subshell's toggle stays in the subshell"
set -o posix
( [[ -o posix ]] && echo "  sub on" || echo "  sub off"
  set +o posix
  [[ -o posix ]] && echo "  sub on" || echo "  sub off" )
[[ -o posix ]] && echo "  parent on" || echo "  parent off"
set +o posix

echo "=== shopt -o reaches the same switch"
shopt -o posix
shopt -qo posix; echo "  q rc=$?"
shopt -so posix; shopt -qo posix; echo "  after -s q rc=$?"
echo "  [${POSIXLY_CORRECT-unset}]"
shopt -uo posix; shopt -qo posix; echo "  after -u q rc=$?"
echo "  [${POSIXLY_CORRECT-unset}]"

echo "=== SHELLOPTS lists posix in its sorted place"
set -o posix -o pipefail
echo "  [$SHELLOPTS]"
set +o posix +o pipefail
echo "  [$SHELLOPTS]"

echo "=== a prefix on a special builtin outlives the command in posix mode"
A=1 eval ':'; echo "  off: A=[${A-unset}]"
set -o posix
A=1 eval ':'; echo "  on:  A=[${A-unset}]"
declare -p A
B=1 : ; echo "  B=[${B-unset}]"
C=1 export D=2; echo "  C=[${C-unset}] D=[${D-unset}]"
E=1 shift 0; echo "  E=[${E-unset}]"
set +o posix
unset A B C D E

echo "=== but not on a regular builtin or a function"
set -o posix
F=1 true; echo "  F=[${F-unset}]"
G=1 command : ; echo "  G=[${G-unset}]"
fn() { :; }
H=1 fn; echo "  H=[${H-unset}]"
set +o posix
unset F G H

echo "=== the persisted value goes in through the ordinary assignment path"
set -o posix
declare -i N=9
N=3+4 eval ':'
declare -p N
declare -x S=old
S=new eval ':'
declare -p S
set +o posix
unset N S

echo "=== an unset inside the command persists nothing"
set -o posix
U=outer
U=inner eval 'unset U; echo "  in=[${U-unset}]"'
echo "  after=[${U-unset}]"
set +o posix
unset U

echo "=== a local under the prefix takes the value"
set -o posix
lp() { local L=loc; L=pfx eval ':'; echo "  in=[$L]"; }
L=glob; lp; echo "  out=[$L]"
set +o posix
unset L

echo "=== the prefix that turns posix mode on keeps itself"
POSIXLY_CORRECT=1 eval ':'
[[ -o posix ]] && echo "  on" || echo "  off"
echo "  [${POSIXLY_CORRECT-unset}]"
unset POSIXLY_CORRECT
POSIXLY_CORRECT=1 true
echo "  after a regular builtin: [${POSIXLY_CORRECT-unset}]"
unset POSIXLY_CORRECT

echo "=== SHELLOPTS is still readonly"
SHELLOPTS=x
echo "  rc=$?"
# In posix mode the rejection is fatal — a variable-assignment error ends a
# non-interactive shell — so a subshell is what keeps this script alive to
# report it. See
# a-shell-diagnostic-can-end-the-shell-under-errexit-or-posix-mode.sh.
set -o posix
( SHELLOPTS=x; echo "  UNREACHABLE" )
echo "  posix rc=$?"
set +o posix
