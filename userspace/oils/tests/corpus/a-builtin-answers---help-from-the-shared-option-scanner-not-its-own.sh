# No builtin has a `--help` of its own. The word is caught one level down, in
# the option scanner they share, at the moment it is about to read a fresh
# option word — and what it prints is not a private message but the very text
# `help <name>` prints, on standard output, with the status a *usage error*
# carries. So the request is answered and refused at once: rc 2, and in posix
# mode a special builtin asked for help ends the shell exactly as a bad option
# would.
#
# Because the test lives in the scan and not in the builtin, "where does
# `--help` count?" is answered by how far the scan has got. After other flags
# it is still an option word; past a `--`, or past the first operand, the scan
# has stopped and the word is an ordinary argument; and a flag that takes an
# argument reaches for the next word without looking at it, so `-d --help`
# spends the request as a delimiter. A flag whose argument is *optional* will
# not take an option-shaped word, so the request behind it survives.
#
# Only the exact spelling counts. `--helpx`, `---help` and `-help` are not it —
# they are option groups, and are read as such, letter by letter, and refused.
#
# A handful of builtins ask the question once instead, of the first word only,
# before their own hand-written parsing: for those `-l --help` is not a request
# but an argument to `-l`. And the few with no option scanner in front of them
# at all — `:`, `true`, `false`, `echo`, `test`, `[` — have no `--help`: the
# word just arrives as an operand.

echo "=== the request is the text help prints, on stdout, with rc 2"
cd --help > a.txt 2> b.txt; echo "  rc=$?"
help cd > c.txt
cmp -s a.txt c.txt && echo "  same text as 'help cd'"
echo "  stderr: [$(cat b.txt)]"
head -1 a.txt
rm -f a.txt b.txt c.txt

echo "=== the name printed is the one the command was called by"
typeset --help | head -1
declare --help | head -1
. --help | head -1
source --help | head -1

echo "=== after other flags it is still an option word"
cd -L --help | head -1
declare +x --help | head -1
read -t 1 --help | head -1
echo "=== and operands may follow it"
cd --help x | head -1

echo "=== past -- or past the first operand it is an ordinary word"
( cd -- --help ) >/dev/null 2>&1; echo "  cd -- --help  rc=$?"
( cd x --help ) >/dev/null 2>&1; echo "  cd x --help   rc=$?"

echo "=== only the exact spelling counts"
for w in --helpx ---help -help --hel; do
  ( cd "$w" ) >/dev/null 2>&1; echo "  cd $w rc=$?"
done

echo "=== a required argument takes the next word without looking at it"
mapfile -d , --help | head -1
mapfile -d --help </dev/null; echo "  mapfile -d --help rc=$?"

echo "=== an optional one leaves an option-shaped word alone"
ulimit -f --help | head -1
set -o --help | head -1

echo "=== an invalid option ends the scan before the request"
( type -z --help ) >/dev/null 2>&1; echo "  rc=$?"

echo "=== some builtins ask only of the first word"
kill --help | head -1
shift --help | head -1
let --help | head -1
dirs --help | head -1
eval --help | head -1
times --help | head -1
getopts --help | head -1
( kill -l --help ) >/dev/null 2>&1; echo "  kill -l --help rc=$?"
( let 1 --help ) >/dev/null 2>&1; echo "  let 1 --help   rc=$?"
echo "  dirs -l --help stdout: [$( { dirs -l --help; } 2>/dev/null )]"

echo "=== and a few have no --help at all"
echo --help
: --help; echo "  :     rc=$?"
true --help; echo "  true  rc=$?"
false --help; echo "  false rc=$?"
test --help; echo "  test  rc=$?"
[ --help ]; echo "  [     rc=$?"

echo "=== bind runs the scan but has no arm to catch the request"
# So it falls through to the same `builtin_usage ()` a bad option would reach —
# the synopsis on stderr, with no "invalid option" line and no long doc.
( bind --help ) 2>&1 | tail -1
( bind -l --help >/dev/null ) 2>&1 | tail -1
( bind --help ) >/dev/null 2>&1; echo "  rc=$?"

echo "=== every builtin, asked plainly"
for b in : true false cd pwd pushd popd dirs echo printf export declare typeset \
         local readonly shopt unset set shift getopts mapfile readarray command \
         builtin read test '[' let eval source . type trap jobs kill wait disown \
         fg bg caller times suspend bind hash history fc umask ulimit exec exit \
         logout return break continue enable alias unalias help compgen complete \
         compopt; do
  # Both runs are in subshells of their own: `exec`'s redirections are the
  # *shell's* when it has no command to run, so an `exec --help >/dev/null` at
  # this level would send the rest of the sweep to /dev/null for good.
  o=$( "$b" --help </dev/null 2>/dev/null | head -1 )
  ( "$b" --help </dev/null ) >/dev/null 2>&1
  printf '  %-10s rc=%-2s [%.44s]\n' "$b" "$?" "$o"
done

echo "=== the status is the usage-error one, so posix mode makes it fatal"
( set -o posix; eval --help >/dev/null 2>&1; echo unreachable ); echo "  rc=$?"
( set -o posix; command eval --help >/dev/null 2>&1; echo reached ); echo "  rc=$?"
( eval --help >/dev/null 2>&1; echo reached ); echo "  rc=$?"
