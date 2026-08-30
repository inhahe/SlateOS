# `set -C` refuses to truncate an existing regular file. What counts as "a form
# that truncates" is wider than the plain `>`: `&> file` is a `> file 2>&1`, so
# it is guarded too, and so is the `>& file` that means the same thing. The
# escapes are the ones that always were — `>|` overrides the option by
# definition, and the append forms have nothing to truncate.
#
# The check is on the *file*, not the word, so it fires the same way through an
# expansion, and it does not fire at all when the target does not exist yet or
# is not a regular file.

mkdir d && cd d || exit 1
new() { rm -f "$1"; : > "$1"; }

echo "=== the plain forms, for comparison"
new f; echo x > f;  echo "  > rc=$?"
new f; echo x >| f; echo "  >| rc=$?"
new f; echo x >> f; echo "  >> rc=$?"
set -C
new f; echo x > f;  echo "  -C > rc=$?"
new f; echo x >| f; echo "  -C >| rc=$?"
new f; echo x >> f; echo "  -C >> rc=$?"

echo "=== and both spellings of 'send everything there'"
new f; echo x &> f;   echo "  &> rc=$?"
new f; echo x &>> f;  echo "  &>> rc=$?"
new f; echo x >& f;   echo "  >& rc=$?"
new f; echo x 1>& f;  echo "  1>& rc=$?"

echo "=== exec takes the same route"
new f; ( exec &> f );  echo "  exec &> rc=$?"
new f; ( exec >& f );  echo "  exec >& rc=$?"
new f; ( exec &>> f ); echo "  exec &>> rc=$?"
new f; ( exec > f );   echo "  exec > rc=$?"
new f; ( exec >| f );  echo "  exec >| rc=$?"

echo "=== it is the file that is checked, not the word"
v=f
new f; echo x &> $v; echo "  &> \$v rc=$?"
new f; echo x >& $v; echo "  >& \$v rc=$?"
rm -f gone
echo x &> gone; echo "  a name with no file yet: rc=$? holds [$(cat gone)]"
echo x &> gone; echo "  …but only the first time: rc=$?"

echo "=== and a descriptor is not a file at all"
echo x >&2 2> e; echo "  >&2 rc=$? e=[$(cat e)]"
new f; echo x 2>& f; echo "  2>& f rc=$? (a dup word, but on fd 2 it is ambiguous)"

echo "=== the option going away lets it all through again"
set +C
new f; echo x &> f; echo "  &> rc=$?"
new f; echo x >& f; echo "  >& rc=$?"
