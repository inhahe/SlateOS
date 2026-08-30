# A syntax error inside a `$( … )` / `<( … )` body is not an ordinary syntax
# error. bash's parser recursed into the body in the middle of reading a word,
# and there is no way back through that word: it jumps to the top level *past*
# the handler `eval` and `.`/`source` install, so the whole shell — or, inside
# one, the enclosing subshell — goes with it. An ordinary syntax error in the
# same place ends only the string being read.
#
# Every fatal probe therefore runs inside a subshell, which is what absorbs the
# unwind and lets the file keep going; its status is the interesting part.

echo "=== an ordinary syntax error ends only the string it was read from"
eval 'for'; echo "  status=$?, and the caller is still running"
eval 'echo one; for'; echo "  status=$?"

echo "=== a substitution body's error takes the caller with it"
( eval 'echo $( ! )'; echo "  NOT REACHED" ); echo "  subshell=$?"
( eval 'echo $(for)'; echo "  NOT REACHED" ); echo "  subshell=$?"
( eval 'cat <( ! )'; echo "  NOT REACHED" ); echo "  subshell=$?"

echo "=== ... past a function, and past a second eval"
f() { eval 'echo $( ! )'; echo "  NOT REACHED"; }
( f; echo "  NOT REACHED" ); echo "  subshell=$?"
( eval 'eval "echo \$( ! )"'; echo "  NOT REACHED" ); echo "  subshell=$?"

echo "=== ... and past a sourced file, which the diagnostic names"
cat > inner.sh <<'INNER'
echo "  inner-one"
echo $( ! )
echo "  NOT REACHED"
INNER
( . ./inner.sh; echo "  NOT REACHED" ); echo "  subshell=$?"

echo "=== ... while a plain syntax error in one is still just its own end"
cat > plain.sh <<'INNER'
echo "  inner-one"
for
echo "  NOT REACHED"
INNER
. ./plain.sh; echo "  status=$?, and the caller is still running"

echo "=== what dies is the nearest subshell, not always the shell"
x=$(eval 'echo $( ! )'); echo "  x=[$x] status=$?"
eval 'echo $( ! )' | cat; echo "  pipeline=$?"
echo "  ... and here we still are"

echo "=== the error is the body's, so a good body is unaffected"
( eval 'echo A$( echo x )B'; echo "  reached=$?" ); echo "  subshell=$?"
