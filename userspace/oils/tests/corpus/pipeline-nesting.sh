# A pipeline is not always the outermost one. When a stage is a shell function,
# a brace group, or a subshell whose *body is itself a pipeline*, the inner
# pipeline's head must read the outer pipe and its tail must write to the outer
# pipe — not the shell's real stdin/stdout. osh used to hard-code "inherit" at
# both ends, which silently dropped the data (stdin already at EOF) or hung
# forever (stdin a terminal). Every shape below is a pipeline nested inside a
# pipeline, exercised through both of osh's executors: the all-external one and
# the threaded one that runs in-process stages.

echo "=== a nested pipeline reads the outer pipe ==="
h() { cat | cat; }
printf 'ab' | h; echo " fn=$?"
printf 'ab' | { cat | cat; }; echo " brace=$?"
printf 'ab' | ( cat | cat ); echo " subsh=$?"
# Two deep: `outer`'s body is a pipeline whose head is itself a function whose
# body is a pipeline.
inner() { cat | cat; }
outer() { inner | cat; }
printf 'ab' | outer; echo " deep=$?"

echo "=== a nested pipeline writes to the outer pipe ==="
# The inner tail is an *external* (`cat`), so the inner pipeline runs through
# the all-external executor while its output must still reach the outer stage.
printf 'ab\n' | inner | tr a-z A-Z
printf 'ab\n' | { cat | cat; } | tr a-z A-Z
# …and as a non-final outer stage feeding a further stage.
printf 'ab\n' | outer | tr a-z A-Z | cat

echo "=== in-process stages inside the nesting ==="
# A builtin tail forces the threaded executor for the inner pipeline.
b() { cat | while read -r l; do echo "[$l]"; done; }
printf 'x\ny\n' | b
# A builtin head, too.
c() { while read -r l; do echo "<$l>"; done | cat; }
printf 'p\nq\n' | c

echo "=== the pipeline's own input is not the script's stdin ==="
# A here-string/here-doc gives the enclosing command a byte cursor rather than a
# real fd; the nested pipeline's head has to see those bytes.
f() { cat | wc -c; }
f <<< 'hello'
g() { cat | cat; }
g <<EOF
one
two
EOF
# Command substitution captures the outer pipeline, whose stages nest again.
echo "sub=$(printf 'axbxc' | { tr -d x | cat; })"

echo "=== nesting does not defeat early termination ==="
# An unbounded producer must stop when a downstream stage closes its input,
# even with a pipeline boundary in between. If this hangs, the fix regressed.
t() { head -c 2 | cat; }
yes ab | t; echo " stop=$?"

echo "=== exit status comes from the nested tail ==="
s() { cat | false; }
printf 'ab' | s; echo "st=$?"
s2() { cat | true; }
printf 'ab' | s2; echo "st=$?"

echo done
