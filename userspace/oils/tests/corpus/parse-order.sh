# The shell reads, parses, and executes one *logical line* at a time. That
# ordering is observable in several ways, and the syntax error at the end of this
# case proves the last of them (so it must stay last — everything after a syntax
# error is abandoned).

# A function body is parsed when the definition is read, so a later redefinition
# of something it calls is picked up at *call* time, not definition time.
helper() { echo helper-v1; }
caller() { helper; }
helper() { echo helper-v2; }
caller

# `eval` parses its argument only when it runs, so it sees state built after the
# enclosing line was parsed.
n=1
eval 'echo eval-sees=$n'
n=2
eval 'echo eval-sees=$n'

# A here-document body belongs to the line that introduced it, so the whole
# construct is one parse unit even though it spans several physical lines.
cat <<EOF
heredoc-line-1
heredoc-$n
EOF
echo "after-heredoc=$?"

# A compound command spanning physical lines is likewise a single unit: the
# parser keeps reading until `done`, and nothing inside it runs early.
for i in a b; do
  echo "loop=$i"
done

# `$(…)` bodies are parsed when the enclosing word is expanded.
f() { echo from-func; }
echo "sub=$(f)"

# Commands before a syntax error have already run and are not undone — but
# nothing on the offending line runs, not even the part before the bad token.
echo before-error
echo not-run-1; echo not-run-2 )
echo unreachable
