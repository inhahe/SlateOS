# Each of the three sections of a `for (( … ))` header is a word expansion, so
# any of them may run commands — and each leaves its own status behind in `$?`.
# None of that is the loop's answer: bash's `execute_arith_for_command`
# (execute_cmd.c) keeps a `body_status`, starts it at `EXECUTION_SUCCESS`, and
# writes it only from `execute_command (arith_for_command->action)`. Whatever
# the header did afterwards is overwritten by the return.
#
# So a loop that never iterates is a success however its header was spelled,
# and a loop that did iterate answers with the last thing its *body* ran — even
# though the update section ran after it.
#
# The two ways the header can fail are not the same as the header merely
# leaving a status: an arithmetic error in the init abandons the command
# outright, and one in the test or step ends the loop with `EXECUTION_FAILURE`.
#
# Verified against bash 5.2.37.

p() { printf '%-46s -> [%s]\n' "$1" "$2"; }

echo "=== a header's own commands are not the answer ==="
for (( $(exit 7); 0; 0 )); do :; done; p 'substitution in init, never iterates' "$?"
for (( 0; $(exit 7) 0; 0 )); do :; done; p 'substitution in test, never iterates' "$?"
for (( i=0; i<1; i=i+$(exit 7)1 )); do :; done; p 'substitution in step' "$?"
for (( $(exit 7); 1; 0 )); do break; done; p 'substitution in init, one pass' "$?"
# The status standing before the loop is not it either.
false; for (( 0; 0; 0 )); do :; done; p 'false before a loop that never runs' "$?"
false; for (( i=0; i<1; i++ )); do :; done; p 'false before a loop that does' "$?"

echo "=== the body is ==="
for (( i=0; i<2; i++ )); do false; done; p 'body ends false' "$?"
for (( i=0; i<2; i++ )); do false; true; done; p 'body ends true' "$?"
for (( i=0; i<2; i++ )); do false; break; done; p 'break after false' "$?"
for (( i=0; i<3; i++ )); do false; continue; done; p 'continue after false' "$?"
# The step runs after the body on every pass but the last, and its status is
# discarded just the same.
for (( i=0; i<2; i=i+$(exit 7)1 )); do true; done; p 'substitution in step, body true' "$?"

echo "=== a header that fails arithmetically is another matter ==="
( for (( 1+; 0; 0 )); do :; done; p 'after a bad init' "$?" ) 2>&1
( for (( 0; 1+; 0 )); do :; done; p 'after a bad test' "$?" ) 2>&1
( for (( 0; 1; 1+ )); do :; done; p 'after a bad step' "$?" ) 2>&1

echo "=== and it reads the same from inside a function ==="
f() { for (( $(exit 7); 0; 0 )); do :; done; }
f; p 'function whose loop never iterates' "$?"
g() { for (( i=0; i<1; i++ )); do false; done; }
g; p 'function whose body ends false' "$?"

echo done
