# POSIX says a failure in a *special* builtin ends a non-interactive shell, and
# bash obeys that only in posix mode. It does not apply the rule to every
# non-zero status — `eval '(exit 2)'`, `shift 99` and `trap x NOSUCHSIG` all
# fail without ending anything — but to four specific failure *classes*:
#
#   * a redirection on the command that cannot be opened;
#   * an assignment refused for naming a readonly variable, whether the builtin
#     made it (`export R=x`) or it was written as a prefix (`R=x eval :`);
#   * a `.`/`source` that cannot open its file;
#   * calling the builtin *wrongly* — an invalid option, a `return` with no
#     function to leave, a parse error in `eval`'s string.
#
# The first two are decided from the command word as written, so `command` and
# `builtin` alike take them off (neither is itself a special builtin). The third
# is decided from bash's `executing_command_builtin` instead, so `builtin` does
# *not* spare it while `command` spares everything running inside it. The first
# three report status 1, in a script and under `-c` alike — except the
# assignment prefix, which bash ends through the same variable-assignment abort
# a bare assignment takes (see
# a-shell-diagnostic-can-end-the-shell-under-errexit-or-posix-mode.sh).
#
# The fourth is the odd one: it reports 2, and it is the only one that a
# suppression context can call off — see its own section below.
#
# Each case runs in a subshell, because a subshell only ever loses itself — that
# is what lets one script exercise forty aborts and still reach the end. `rc=` is
# the subshell's status, so 1 means "the body stopped". The last section is the
# one case at top level, proving the abort really does end the *script*.

r() { ( eval "$2" ) 2>&1; echo "  $1 rc=$?"; }

echo "=== a redirection that cannot be opened, on a special builtin"
r colon 'set -o posix; : > /nope/f; echo ran'
r exec 'set -o posix; exec 9< /nope/f; echo ran'
r eval 'set -o posix; eval : > /nope/f; echo ran'
r export 'set -o posix; export X=1 > /nope/f; echo ran'
r unset 'set -o posix; unset X > /nope/f; echo ran'
r trap 'set -o posix; trap -p > /nope/f; echo ran'
r shift 'set -o posix; shift 0 > /nope/f; echo ran'
r times 'set -o posix; times > /nope/f; echo ran'
r exit 'set -o posix; exit 2> /nope/f; echo ran'
r source 'set -o posix; . /dev/null > /nope/f; echo ran'

echo "=== …and on nothing else"
r regular-builtin 'set -o posix; true > /nope/f; echo ran'
r external 'set -o posix; echo hi > /nope/f; echo ran'
r brace-group 'set -o posix; { :; } > /nope/f; echo ran'
r function 'set -o posix; f() { :; }; f > /nope/f; echo ran'
r null-command 'set -o posix; x=1 > /nope/f; echo ran'
r under-command 'set -o posix; command : > /nope/f; echo ran'
r under-builtin 'set -o posix; builtin : > /nope/f; echo ran'
r without-posix ': > /nope/f; echo ran'

echo "=== an assignment refused for naming a readonly variable"
r export 'set -o posix; readonly R=1; export R=x; echo ran'
r readonly 'set -o posix; readonly R=1; readonly R=x; echo ran'
r export-array-flag 'set -o posix; readonly R=1; export -a R=x; echo ran'
r prefix-eval 'set -o posix; readonly R=1; R=x eval :; echo ran'
r prefix-colon 'set -o posix; readonly R=1; R=x :; echo ran'

echo "=== …which both prefixes take off, and which spares a regular builtin"
r under-command 'set -o posix; readonly R=1; command export R=x; echo ran'
r under-builtin 'set -o posix; readonly R=1; builtin export R=x; echo ran'
r prefix-regular 'set -o posix; readonly R=1; R=x true; echo ran'
r declare 'set -o posix; readonly R=1; declare R=x; echo ran'
r attribute-only 'set -o posix; readonly R=1; export R; echo ran'
r without-posix 'readonly R=1; export R=x; echo ran'

echo "=== a . / source that cannot open its file"
r dot 'set -o posix; . /nope; echo ran'
r source 'set -o posix; source /nope; echo ran'
# This class asks whether a `command` is *running*, not whether the `.` was
# reached through one — so a `command` anywhere up the stack spares it, and a
# `builtin` prefix does not spare it at all.
r under-builtin 'set -o posix; builtin . /nope; echo ran'
r under-command 'set -o posix; command . /nope; echo ran'
r inside-command 'set -o posix; command eval ". /nope"; echo ran'
r without-posix '. /nope; echo ran'

echo "=== no suppression context spares any of the three"
r or-true 'set -o posix; . /nope || true; echo ran'
r and-true 'set -o posix; : > /nope/f && true; echo ran'
r bang 'set -o posix; ! . /nope; echo ran'
r if-cond 'set -o posix; if . /nope; then :; fi; echo ran'
r while-cond 'set -o posix; while . /nope; do break; done; echo ran'
r in-function 'set -o posix; f() { . /nope; }; f; echo ran'
r in-eval 'set -o posix; eval ". /nope"; echo ran'
r err-trap 'set -o posix; trap "echo T" ERR; . /nope; echo ran'
r assignment-or-true 'set -o posix; readonly R=1; export R=x || true; echo ran'

echo "=== the fourth class: calling a special builtin wrongly"
# Status 2 here, not 1. Every one of these is an invalid option except the last
# three, which are the other shapes the class takes.
r unset 'set -o posix; unset -q; echo ran'
r export 'set -o posix; export -q; echo ran'
r readonly 'set -o posix; readonly -q; echo ran'
r set 'set -o posix; set -q; echo ran'
r set-o 'set -o posix; set -o nosuchopt; echo ran'
r set-plus-o 'set -o posix; set +o nosuchopt; echo ran'
r trap 'set -o posix; trap -q; echo ran'
r times 'set -o posix; times -q; echo ran'
r eval 'set -o posix; eval -q; echo ran'
r exec 'set -o posix; exec -q; echo ran'
r exit 'set -o posix; exit -q; echo ran'
r dot 'set -o posix; . -q; echo ran'
r dot-no-file 'set -o posix; .; echo ran'
r return-outside 'set -o posix; return; echo ran'
r eval-parse-error 'set -o posix; eval "syntax ( error"; echo ran'

echo "=== …which spares a regular builtin, and which both prefixes take off"
r declare 'set -o posix; declare -Q; echo "rc=$?"; echo ran'
r read 'set -o posix; read -Q; echo "rc=$?"; echo ran'
r local 'set -o posix; local -q; echo "rc=$?"; echo ran'
r under-command 'set -o posix; command unset -q; echo "rc=$?"; echo ran'
r under-builtin 'set -o posix; builtin unset -q; echo "rc=$?"; echo ran'
r operand-not-option 'set -o posix; shift -q; echo "rc=$?"; echo ran'
r without-posix 'unset -q; echo "rc=$?"; echo ran'

echo "=== and unlike the other three, a suppression context does spare it"
# bash checks a flag at the simple-command node, and only when that node's
# status is going to be *used* — so a non-final `&&`/`||` operand or a compound
# condition calls the abort off. The suppression is dynamic, so it reaches into
# a function body; `!` is node-local, so it spares only the node it is written
# on. `eval` parses a fresh command list, so an enclosing suppression stops at
# its edge while one written inside it applies.
r or-true 'set -o posix; unset -q || true; echo ran'
r and-true 'set -o posix; unset -q && true; echo ran'
r final-operand 'set -o posix; true && unset -q; echo ran'
r bang 'set -o posix; ! unset -q; echo ran'
r bang-group 'set -o posix; ! { unset -q; }; echo ran'
r bang-function 'set -o posix; f() { unset -q; }; ! f; echo ran'
r if-cond 'set -o posix; if unset -q; then :; fi; echo ran'
r while-cond 'set -o posix; while unset -q; do break; done; echo ran'
r group 'set -o posix; { unset -q; }; echo ran'
r in-function 'set -o posix; f() { unset -q; }; f; echo ran'
r function-or-true 'set -o posix; f() { unset -q; }; f || true; echo ran'
r function-if-cond 'set -o posix; f() { unset -q; }; if f; then :; fi; echo ran'
r nested-function 'set -o posix; f() { g() { unset -q; }; g || true; }; f; echo ran'
r eval-then-true 'set -o posix; eval "unset -q" || true; echo ran'
r true-inside-eval 'set -o posix; eval "unset -q || true"; echo ran'
# An ERR trap that actually *runs* disarms it: the handler's own simple command
# clears the flag before the check is reached. One that is ignored or reset
# never runs, so it does not.
r err-trap-runs 'set -o posix; trap ":" ERR; unset -q; echo ran'
r err-trap-ignored 'set -o posix; trap "" ERR; unset -q; echo ran'
r err-trap-reset 'set -o posix; trap - ERR; unset -q; echo ran'
r debug-trap 'set -o posix; trap ":" DEBUG; unset -q; echo ran'

echo "=== an ordinary non-zero status is not one of the classes"
r unset-readonly 'set -o posix; readonly R=1; unset R; echo "rc=$?"; echo ran'
r bad-name 'set -o posix; export 1bad=1; echo "rc=$?"; echo ran'
r bad-signal 'set -o posix; trap x NOSUCHSIG; echo "rc=$?"; echo ran'
r shift-past-end 'set -o posix; shift 99; echo "rc=$?"; echo ran'
r break-zero 'set -o posix; break 0; echo "rc=$?"; echo ran'
r eval-exit-2 'set -o posix; eval "(exit 2)"; echo "rc=$?"; echo ran'
# These two look like usage errors but report 1, and bash's check is on the
# status the builtin hands back — so 1 keeps them out of the class.
r unset-f-and-v 'set -o posix; unset -f -v x; echo "rc=$?"; echo ran'
r shift-not-a-number 'set -o posix; shift a; echo "rc=$?"; echo ran'

echo "=== a subshell only ever loses itself"
set -o posix
( . /nope; echo ran ); echo "  sub rc=$?"
echo "  parent still here"
set +o posix

echo "=== and now for real, at top level"
# Everything after this line is unreachable: the failed open ends the script, so
# neither the `echo` on the next line nor the final marker runs.
set -o posix
. /nope
echo "  UNREACHABLE"
echo "=== NOT REACHED"
