# POSIX says a redirection word is expanded without field splitting and without
# pathname expansion, and bash obeys that in posix mode — but not symmetrically.
# Its `redirection_expand` sets only the no-split flag on the word and then
# hands it to a glob-ing expander, so:
#
#   * a *filename* word (`<`, `>`, `>>`, `<>`, `&>`) loses both splitting and
#     globbing, while
#   * a *dup* word (`<&`, `M>&`, `M<&`) loses splitting only, and is still
#     globbed.
#
# What survives in both is null-word removal, which is why an unsplit word is
# still not the same as a joined one: an *unquoted* expansion that produced
# nothing leaves zero fields and stays "ambiguous", while a *quoted* empty one
# is a single empty field that gets as far as the open.
#
# (Names like `[ab]` stand in for the more obvious `g*` because a `*` cannot be
# a filename on every host this runs on.)

mkdir d && cd d || exit 1
: > 1
: > a
: > b
r="two words"
u=

echo "=== outside the mode, a redirection word splits and globs like an argument"
echo hi > $r; echo "  rc=$?"
echo hi > [ab]; echo "  rc=$? (globbed to two names)"
echo hi 2>& $r; echo "  rc=$? (split into two, and it names the word as written)"

set -o posix

echo "=== a filename word no longer splits"
echo hi > $r; echo "  rc=$?"
echo "  'two words' holds: $(cat 'two words')"

echo "=== nor globs — the pattern is the name"
echo lit > [ab]; echo "  rc=$?"
echo "  the file named [ab] holds: $(cat '[ab]')"
echo "  a and b were left alone: [$(cat a)] [$(cat b)]"
cat < [1]; echo "  rc=$? (there is a file called 1, but no glob ran to find it)"

echo "=== but a dup word is still globbed"
echo "  2>& [1] finds fd 1, so this lands on stdout:"
nosuchcommand 2>& [1]; echo "  rc=$?"
echo "  while [ab] globs to two names, which is ambiguous:"
echo hi 2>& [ab]; echo "  rc=$?"
# The two ways to spell "send both streams to a file" part company right here.
# They end up meaning the same thing, but they are different instructions until
# the word has been expanded: `&>` is a filename word, `>&` is a dup one.
echo "  and the two spellings of 'both streams to a file' disagree:"
echo amp &> [ab]; echo "  rc=$? (&> takes the name literally)"
echo "  the file named [ab] now holds: $(cat '[ab]')"
echo gta >& [ab]; echo "  rc=$? (>& globbed it, to two names)"
echo "  a and b are still empty: [$(cat a)] [$(cat b)]"

echo "=== a dup word does not split, though"
echo "  so it is one field, and the ambiguity names the expansion, not the word:"
echo hi 2>& $r; echo "  rc=$?"
cat <& $r; echo "  rc=$?"

echo "=== null-word removal outlives both"
echo hi > $u; echo "  rc=$? (unquoted empty: no field at all)"
echo hi > "$u"; echo "  rc=$? (quoted empty: one field, and the open fails)"
echo hi 2>& $u; echo "  rc=$?"
echo hi 2>& "$u"; echo "  rc=$?"

echo "=== the here-document forms were never routed through any of this"
cat <<< $r
cat <<EOF
$r and [ab]
EOF

echo "=== and it all comes back when posix mode goes"
set +o posix
echo hi > $r; echo "  rc=$?"
echo hi > [ab]; echo "  rc=$?"
