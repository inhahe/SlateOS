# A `$( … )` body is read twice, and the two reads number its lines
# differently.
#
# The *second* read — at expansion time — counts up from the closing `)`'s line
# by rank, which is what `$LINENO` and an expansion-time diagnostic report (see
# cmdsub-incremental.sh). The *first* read happens in the enclosing token
# stream, so it is numbered plainly: a syntax error the enclosing scan raises
# names the body's true physical line and echoes that physical source line back
# — closing `)` and all, because the echo is of the script's line, not of the
# body's.
#
# Such an error is fatal to its reader (see cmdsub-error-fatality.sh), so every
# probe below is a sourced file run inside a subshell, which absorbs the unwind.
#
# See known-issues.md TD-OILS-CMDSUB-EAGER-ERROR-LINE.

run() {
  ( . ./inner.sh; echo "  NOT REACHED" )
  echo "  subshell=$?"
}

echo "=== the error names the body line it is on, not a ranked one"
cat > inner.sh <<'INNER'
echo one
x=$(echo a
echo b
for
echo d)
echo "  NOT REACHED"
INNER
run

echo "=== ... including when it is the body's first line"
cat > inner.sh <<'INNER'
echo one
x=$(for
echo b)
echo "  NOT REACHED"
INNER
run

echo "=== ... and when the body opens on its own line"
cat > inner.sh <<'INNER'
echo one
x=$(
echo a

for
)
echo "  NOT REACHED"
INNER
run

echo "=== a nested body is numbered against the outer body's physical lines"
cat > inner.sh <<'INNER'
echo one
x=$(echo a
echo $(echo b
for
echo c)
echo d)
echo "  NOT REACHED"
INNER
run

echo "=== a body that stops mid-construct is blamed on the closing paren"
cat > inner.sh <<'INNER'
echo one
x=$(echo a
if true
echo b)
echo "  NOT REACHED"
INNER
run

echo "=== a process substitution body numbers from its opening paren"
cat > inner.sh <<'INNER'
echo one
cat <(echo a
echo b
for
echo d)
echo "  NOT REACHED"
INNER
run

echo "=== and the script that got here is still fine"
echo "  [$(echo ok)]"
