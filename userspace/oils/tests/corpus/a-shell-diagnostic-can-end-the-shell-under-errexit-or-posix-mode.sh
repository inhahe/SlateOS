# Most of what goes wrong during expansion or assignment is merely a *discard*:
# the command is abandoned, `$?` becomes 1, and the next top-level parse unit
# still runs. Two options promote some of those into a whole-shell abort, and
# they do not cover the same errors:
#
#   * `set -e`. bash reports these through `report_error()`, which ends with
#     `if (exit_immediately_on_error) exit_shell (…)`. The *diagnostic* is what
#     ends the shell, not the usual failed-command check, so none of errexit's
#     exemptions apply — `|| true`, an `if` condition, `!` and a function body
#     all fail to spare it. The status is always 1.
#   * `set -o posix`. POSIX requires a non-interactive shell to exit on an
#     expansion or variable-assignment error. This needs no errexit and carries
#     the word-expansion abort status (1 in a script, 127 under `-c`).
#
# Each case below runs in a subshell, because a subshell only ever loses
# itself — that is what lets one script exercise forty aborts and still reach
# the end. `rc=` is the subshell's status, so 1 means "the body stopped", and
# the diagnostic on stderr is compared too. The very last section is the one
# case at top level, proving a promoted abort really does end the *script*.

r() { ( eval "$2" ) 2>&1; echo "  $1 rc=$?"; }

echo "=== neither option: a diagnostic, and the shell carries on"
r bare-readonly 'readonly R=1; R=x; echo ran'
r prefix-readonly 'readonly R=1; R=x true; echo ran'
r read-subscript 'a=1; echo "[${a[-9]}]"; echo ran'
r arith 'echo $((1+)); echo ran'
r badsub 'echo "${x!y}"; echo ran'

echo "=== errexit: the diagnostic itself ends the shell"
r bare-readonly 'set -e; readonly R=1; R=x; echo ran'
r prefix-readonly 'set -e; readonly R=1; R=x true; echo ran'
r for-readonly 'set -e; readonly R=1; for R in a; do :; done; echo ran'
r read-subscript 'set -e; a=1; echo "[${a[-9]}]"; echo ran'
r len-subscript 'set -e; a=(1); echo "[${#a[-9]}]"; echo ran'
r assign-subscript 'set -e; a=(1); a[-9]=v; echo ran'
r assoc-empty-key 'set -e; declare -A m; m[""]=v; echo ran'
r bad-indirect 'set -e; echo "${!1x}"; echo ran'
r bad-param 'set -e; echo "${1x}"; echo ran'
r badsub 'set -e; echo "${x!y}"; echo ran'

echo "=== errexit's own exemptions do not spare it"
r or-true 'set -e; readonly R=1; R=x || true; echo ran'
r if-cond 'set -e; if readonly R=1; R=x; then :; fi; echo ran'
r bang 'set -e; ! { readonly R=1; R=x; }; echo ran'
r in-function 'set -e; f() { readonly R=1; R=x; }; f; echo ran'

echo "=== but errexit does not reach every expansion error"
r arith 'set -e; echo $((1+)); echo ran'
r divide-by-zero 'set -e; echo $((1/0)); echo ran'
r substring 'set -e; a=abc; echo "[${a:0:-5}]"; echo ran'
r integer-attr 'set -e; declare -i n; n=1+; echo ran'
r assign-in-expansion 'set -e; a=(1 2); echo "${a[@]:=v}"; echo ran'

echo "=== posix mode, with no errexit at all"
r bare-readonly 'set -o posix; readonly R=1; R=x; echo ran'
r for-readonly 'set -o posix; readonly R=1; for R in a; do :; done; echo ran'
r assign-subscript 'set -o posix; a=(1); a[-9]=v; echo ran'
r assoc-empty-key 'set -o posix; declare -A m; m[""]=v; echo ran'
r bad-indirect 'set -o posix; echo "${!1x}"; echo ran'
r bad-param 'set -o posix; echo "${1x}"; echo ran'
r badsub 'set -o posix; echo "${x!y}"; echo ran'
r arith 'set -o posix; echo $((1+)); echo ran'
r divide-by-zero 'set -o posix; echo $((1/0)); echo ran'

echo "=== the two rules cover different errors"
# Fatal under errexit, a plain diagnostic under posix mode: reading through a
# bad subscript, and a readonly rejection of an assignment *prefix*.
r read-subscript 'set -o posix; a=1; echo "[${a[-9]}]"; echo ran'
r len-subscript 'set -o posix; a=(1); echo "[${#a[-9]}]"; echo ran'
r prefix-readonly 'set -o posix; readonly R=1; R=x true; echo ran'
# Fatal under posix mode, a plain discard under errexit: arithmetic.
r integer-attr 'set -o posix; declare -i n; n=1+; echo ran'
r substring 'set -o posix; a=abc; echo "[${a:0:-5}]"; echo ran'

echo "=== a subshell only ever loses itself"
set -e
( readonly R=1; R=x; echo ran ) || echo "  sub rc=$?"
set +e
set -o posix
( echo $((1+)); echo ran ); echo "  sub rc=$?"
echo "  parent still here"
set +o posix

echo "=== nested one deep, from a function"
r nested 'set -o posix; f() { ( echo $((1+)); echo inner ); echo "  after=$?"; }; f; echo ran'

echo "=== eval does not contain it"
r eval-posix 'set -o posix; eval "echo \$((1+))"; echo ran'
r eval-errexit 'set -e; eval "readonly R=1; R=x"; echo ran'
r eval-plain 'eval "echo \$((1+))"; echo ran'

echo "=== a command substitution is a subshell like any other"
r cmdsub 'set -o posix; v=$(echo $((1+)); echo in); echo "v=[$v] rc=$?"; echo ran'

echo "=== and now for real, at top level"
# Everything after this line is unreachable: the readonly rejection ends the
# script, so neither the `echo` on the next line nor the final marker runs.
set -o posix
readonly TOP=1
TOP=x
echo "  UNREACHABLE"
echo "=== NOT REACHED"
