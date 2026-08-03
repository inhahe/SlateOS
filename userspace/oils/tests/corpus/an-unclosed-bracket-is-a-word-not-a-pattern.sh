# A `[` does not begin a glob on its own. The shell only calls a word a pattern
# once a `]` arrives with a `[` still open — and the `[` is forgotten at every
# `/`, because a pattern is only ever matched against one path component. So
# `[`, `[abc`, `a]b` and `[a/b]` are ordinary words, while `[abc]` and
# `[a]b/c]` are patterns.
#
# Normally this is invisible: an unmatched pattern is left as its own spelling,
# which is the same text a literal would have produced. `nullglob` is what
# separates them — it deletes a pattern that matches nothing and leaves a
# literal alone — so every probe here runs under it.
#
# The reason to care is not the corner itself but its cost. `[` is the name of
# a builtin, so `[ 1 -lt 2 ]` puts one at the head of every test in every loop
# in every script. Reading the whole current directory to discover that `[`
# matches nothing made that word sixteen times more expensive than the
# identical `test`, and the directory's size leaked into the shell's speed. See
# known-issues TD-OILS-UNCLOSED-BRACKET-GLOBBED.
#
# The probes run in a directory of this case's own making, so what is on disk
# is known: `a` is the only name a bracket expression can match, which is how
# the *closed* ones are seen to still glob.
shopt -s nullglob
mkdir -p bdir && cd bdir || exit 1
: > 'a'

echo "=== an unclosed bracket is a word, and stays"
for w in '[' ']' '[abc' 'a]b' ']x[' '[[' ']]'; do
  eval "printf '  %-8s -> <%s>\n' \"\$w\" \"\$(echo $w)\""
done

echo "=== and it does not span a /"
for w in '[a/b]' '[a/b]c]' '[a/b/c]' 'x[a/b]'; do
  eval "printf '  %-8s -> <%s>\n' \"\$w\" \"\$(echo $w)\""
done

echo "=== a closed one is a pattern, in whichever component it closes"
echo "  matches   <$(echo [a])>"
echo "  no match  <$(echo [qz])>"
echo "  later     <$(echo [a]b/c])>"
echo "  subdir    <$(echo nosuchdir/[ab])>"
echo "  star      <$(echo nosuch*zzz)>"

echo "=== a quoted bracket is not one at all"
echo "  quoted    <$(echo '[a]')>"
echo "  escaped ] <$(echo [a\])>"
echo "  escaped [ <$(echo \[a])>"

echo "=== the same word as the head of a command still finds the builtin"
[ 1 -lt 2 ]; echo "  [ rc=$?"
[ 1 -gt 2 ]; echo "  [ rc=$?"
