# `2>&1` is a `dup2`: fd 1 and fd 2 stop being two descriptors and become one
# open file description. Everything written down it is written in the order the
# writes happened, because there is only one thing to write to — the kernel is
# what interleaves them, not the shell.
#
# A shell that captures the two streams separately and appends one to the other
# gets a plausible-looking answer that is wrong the moment the command alternates
# between them:
#
#   echo A >&2; echo B; echo C >&2      dup2:        A B C
#                                       concatenate: B A C   (all of fd 1 first)
#
# Builtins are easy to get right, because the shell is doing the writing and can
# simply write in order. An *external* command is the test: the shell has to hand
# the child one description rather than two, and a `>` written after the `2>&1`
# is the other half of the same question — it takes fd 1 away again, leaving only
# fd 2 on the capture.
#
# `$BASH` is the running shell, so the child below is the shell under test.

f() { "$BASH" --norc -c 'echo A >&2; echo B; echo C >&2'; }
p() { printf '[%s]\n' "$(printf '%s' "$1" | tr '\n' ' ')"; }

echo "=== a 2>&1 written on the external command itself"
p "$( "$BASH" --norc -c 'echo A >&2; echo B; echo C >&2' 2>&1 )"

echo "=== and one written on something enclosing it, which is a different arm"
# A compound's redirect reaches the child as a *scoped* stderr target rather
# than as its own `2>&1`, so it is worth spelling all three enclosures out.
p "$( f 2>&1 )"
p "$( { f; } 2>&1 )"
p "$( ( f ) 2>&1 )"
p "$( { { f; }; } 2>&1 )"

echo "=== with the shell's own writes among them"
p "$( { echo pre; f; echo post; } 2>&1 )"
p "$( { echo pre >&2; f; echo post >&2; } 2>&1 )"

echo "=== builtins alone, which never needed a child to get right"
p "$( { echo A >&2; echo B; echo C >&2; } 2>&1 )"
p "$( ( echo A >&2; echo B; echo C >&2 ) 2>&1 )"

echo "=== and through a real pipe, where the merge was always the kernel's"
p "$( f 2>&1 | cat )"
p "$( { f; } 2>&1 | cat )"

echo "=== a later > takes fd 1 back, leaving fd 2 alone on the capture"
p "$( f 2>&1 >out )"; p "$(cat out)"; rm -f out
p "$( { f; } 2>&1 >out )"; p "$(cat out)"; rm -f out
echo "--- and the other order sends both to the file"
p "$( f >out 2>&1 )"; p "$(cat out)"; rm -f out

echo "=== 2> of its own wins over the capture"
p "$( f 2>err )"; p "$(cat err)"; rm -f err
p "$( f 2>/dev/null )"

echo "=== nested captures each merge their own child"
p "$( echo "inner=$( f 2>&1 )" 2>&1 )"

echo "=== an assignment's capture, and the status that comes with it"
x=$( f 2>&1 ); rc=$?; p "$x"; echo "rc=$rc"
x=$( "$BASH" --norc -c 'echo A >&2; exit 3' 2>&1 ); rc=$?; p "$x"; echo "rc=$rc"

echo "=== more than a pipe buffer's worth, on both streams at once"
# One pipe with one reader cannot deadlock; two would, once either filled while
# the other was being drained. 4000 lines is well past any pipe buffer. Only the
# *count* is asserted here, not the order: with this much traffic the order is
# the child's own buffering, which is not what this case is about — the point is
# that nothing is lost and nothing hangs.
big=$( "$BASH" --norc -c 'for i in $(seq 1 2000); do echo "o$i"; echo "e$i" >&2; done' 2>&1 )
printf 'captured=%s distinct=%s\n' \
  "$(printf '%s\n' "$big" | wc -l)" \
  "$(printf '%s\n' "$big" | sort -u | wc -l)"
echo done
