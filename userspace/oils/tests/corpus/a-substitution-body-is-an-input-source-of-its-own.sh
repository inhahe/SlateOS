# A syntax error is named after the *innermost input source*, and a command
# substitution's body is one.
#
# bash's `command_substitute` runs the body through `parse_and_execute`, which
# pushes an input source called `command substitution`; `report_syntax_error`
# names whatever is on top. So the `eval` or `.` the substitution was written
# inside stops counting the moment the body starts — and an `eval` the body
# itself runs starts counting again, one level further up.
#
# The three deferred spellings all take this path: a backtick, a `$( … )` whose
# body changed its own parse state mid-way (an `alias` defined inside it), and a
# `$((` that failed `chk_arithsub` and fell back. Only the token is at stake —
# `$?` and the fate of the enclosing command are the same either way.
# Verified against bash 5.2.37.

echo "=== a substitution's body is a source of its own ==="
echo `for`; echo "  rc=$?"
x=`for`; echo "  rc=$?"
# The eager `$( … )` parse reaches its own read-eval loop only when the body
# rewrites the grammar under itself, which an alias does.
echo $(alias q=for
q); echo "  rc=$?"

echo "=== …pushed on top of an eval ==="
eval 'echo `for`'; echo "  rc=$?"
eval 'x=`for`; echo "  inner=$?"'; echo "  rc=$?"

echo "=== …and an eval inside it is pushed on top again ==="
echo `eval 'for'`; echo "  rc=$?"
# The inner quotes end the outer ones, so the body is `eval 'echo ` and the
# missing `'` is the substitution's own — still named for it.
echo `eval 'echo `for`'`; echo "  rc=$?"

echo "=== a plain eval still names itself ==="
eval 'for'; echo "  rc=$?"

echo "=== the fallback body is one of these too ==="
echo $(( fi ) ); echo "  rc=$?"
eval 'echo $(( fi ) )'; echo "  rc=$?"

echo "=== …pushed on top of a source, which keeps its own name ==="
cat > lib.sh <<'LIB'
echo `for`; echo "  rc=$?"
eval 'echo `for`'; echo "  rc=$?"
LIB
. ./lib.sh
echo done
