# Process substitution `<( )` / `>( )` and the `coproc` keyword — the two
# constructs that hand a command an *fd path* rather than a captured string.

# `<( )` appears as a readable path; the reader sees the producer's stdout.
cat <(echo from-procsub)
cat <(printf 'a\nb\n') <(printf 'c\n')

# The word itself is a *path*, not the command's output — but its exact shape is
# deliberately not asserted here: bash names an fd (`/dev/fd/63`) while osh names
# a temp file on hosts without `/dev/fd` (TD-OILS22), and by the time the
# assignment has finished neither one is still open.
p=<(echo x)
case "$p" in
  *x*) echo "procsub-word-is-not-the-output=no" ;;
  ?*) echo "procsub-word-is-a-nonempty-path=yes" ;;
esac

# `>( )` is the mirror image: what the command writes goes to the substitution's
# stdin. Wait for it so the output ordering is deterministic.
echo to-writer > >(sed 's/^/wrote:/' > wout.txt)
wait
cat wout.txt

# A while-read loop fed by a process substitution keeps its variables, unlike
# the pipeline form (whose loop body runs in a subshell).
n=0
while read -r line; do n=$((n + 1)); done < <(printf 'p\nq\nr\n')
echo "procsub-loop-count=$n"
n=0
printf 'p\nq\nr\n' | while read -r line; do n=$((n + 1)); done
echo "pipeline-loop-count=$n"

# diff-style two-producer comparison, the canonical use.
if diff <(printf 'same\n') <(printf 'same\n') > /dev/null; then echo diff-equal; fi
if ! diff <(printf 'x\n') <(printf 'y\n') > /dev/null; then echo diff-differs; fi

# Exit status: the *command's* status is what matters, not the substitution's.
false > >(cat > /dev/null)
echo "status-after-writer=$?"
true < <(false)
echo "status-after-reader=$?"

# `coproc` gives a two-way pipe through the fd pair in the named array. Keep it
# to a single round trip: osh's coproc is not fully streaming (TD-OILS22), so a
# probe that depends on interleaving would be testing the limitation, not the
# semantics.
coproc CO { read -r line; echo "echoed:$line"; }
echo to-coproc >&"${CO[1]}"
read -r reply <&"${CO[0]}"
echo "coproc-reply=$reply"
wait "$CO_PID" 2>/dev/null
echo "coproc-done"
