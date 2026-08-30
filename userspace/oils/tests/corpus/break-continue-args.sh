# `break`/`continue`: the loop status they leave behind, and the three ways
# bash rejects their operand. Measured against bash 5.2.
#
#   * both are the last command executed in their iteration, so *their* status
#     — not the previous iteration's — is the loop's;
#   * outside any loop both are no-ops that warn and return 0, and the operand
#     is not even looked at (`break abc` outside a loop is only the warning);
#   * a non-numeric count kills the shell, with `$?` ORed with 128 (so a status
#     that already has that bit keeps its value) and the EXIT trap still run;
#   * a second operand is "too many arguments" — status 1, the rest of the
#     current top-level unit discarded — and it is diagnosed *before* the range
#     check, so `break 0 2` reports that rather than the range error;
#   * a zero/negative count leaves *every* enclosing loop — for `continue` just
#     as much as for `break` — with status 1;
#   * `--` ends option processing, and the count itself may carry a sign and
#     surrounding whitespace.
#
# The error lines name the shell, whose spelling differs per host, so every
# probe folds the leading "<shell>: line N: " away. Each probe runs in a
# subshell (the fatal cases would otherwise take the test with them) and its
# status is captured through a file, because a pipeline would report `sed`'s.
sq() { sed 's/^[^ ]*: line [0-9]*: /SH: /' "$1"; }
run() { ( eval "$1" ) >o.txt 2>&1; echo "rc=$?"; sq o.txt; }

echo "=== the loop's status is break's, not the last iteration's"
for i in 1 2 3; do (exit 4); [ $i = 3 ] && break; done
echo "and=$?"
for i in 1 2; do (exit 5); break; done
echo "plain=$?"
for i in 1 2; do break; (exit 5); done
echo "before=$?"
i=0; while [ $i -lt 3 ]; do i=$((i+1)); (exit 6); [ $i = 2 ] && break; done
echo "while=$?"
for i in 1 2; do (exit 7); continue; done
echo "continue=$?"
for a in 1 2; do for b in x y; do (exit 8); break 2; done; done
echo "break2=$?"

echo "=== outside a loop the operand is ignored"
run 'break abc; echo "after=$?"'
run 'continue 0; echo "after=$?"'

echo "=== a non-numeric count kills the shell"
run 'for i in 1 2; do break abc; done; echo NOT-REACHED'
run '(exit 3); for i in 1; do break 2abc; done'
run '(exit 130); for i in 1; do break xyz; done'
run 'trap "echo EXITTRAP" EXIT; for i in 1; do continue ""; done'
run 'for i in 1; do break 0x10; done'
run 'for i in 1; do break 99999999999999999999999; done'

echo "=== a second operand is too many arguments, checked first"
run 'for i in 1 2; do break 1 2; done; echo NOT-REACHED'
run 'for i in 1 2; do break 0 2; done; echo NOT-REACHED'
run 'for i in 1 2; do continue 1 2; done; echo NOT-REACHED'
# Only the rest of the *current* top-level unit is discarded, so a following
# line still runs. Run this one at the script's own top level: `run` would
# wrap it in an `eval`, whose discard scope is a separate question.
for i in 1 2; do break 1 2; done 2>o.txt
echo "next-line-runs=$?"; sq o.txt

echo "=== zero or negative leaves every enclosing loop"
run 'for a in 1 2; do for b in x y; do break 0; done; echo "inner=$?"; done; echo "outer=$?"'
run 'for a in 1 2; do for b in x y; do continue -1; done; echo "inner=$?"; done; echo "outer=$?"'

echo "=== -- and a signed, padded count"
for a in 1 2; do for b in x y; do break -- 2; done; echo "inner=$?"; done
echo "dashdash2=$?"
for i in 1 2; do break --; echo "not-reached"; done
echo "bare=$?"
for a in 1 2; do for b in x y; do break " +2 "; done; echo "inner=$?"; done
echo "padded=$?"
for a in 1 2; do for b in x y; do break 9; done; echo "inner=$?"; done
echo "clamped=$?"
