# bash's parser sorts a `<&`/`>&` target into one of three instructions by
# looking at the *literal* word, before any expansion:
#
#   * a bare `-`           → close this descriptor
#   * a bare run of digits → duplicate that descriptor
#   * anything else        → a "dup word": decided later, once expanded
#
# The third is the interesting one. `>& file` ends up meaning exactly what
# `&> file` means — both streams to the file — but bash does not decide that
# until redirection time, because until the word is expanded `>& $v` cannot be
# told apart from `>& 2`. Keeping the instruction distinct that long is what
# this case is about: how the redirect prints back, and that the rewrite still
# happens everywhere the redirect is applied. Its other consequence — posix mode
# globs a dup word but not a filename one — has a case of its own; see
# posix-mode-stops-splitting-a-redirection-word-but-still-globs-a-dup.sh.
#
# The printer treats the three differently too: a close always comes out as an
# *output* dup with its fd shown (`<&-` prints `0>&-`), a number always shows
# its fd (`>&2` prints `1>&2`), and only a dup word may leave the operator's
# default fd off (`>& out` prints `>&out`, but `2>& out` prints `2>&out`).

echo "=== how each of the three prints back"
close_in()   { cat <&-; }
close_out()  { echo x >&-; }
close_three(){ cat 3>&-; }
close_err()  { echo x 2>&-; }
num_bare()   { echo x >&2; }
num_fd()     { echo x 1>&2; }
num_err()    { echo x 2>&1; }
word_bare()  { echo x >& out; }
word_one()   { echo x 1>& out; }
word_err()   { echo x 2>& out; }
word_in()    { cat <& in; }
word_in_fd() { cat 3<& in; }
# Quoting a number is enough to make it a word rather than a descriptor — the
# parser never looks through quotes — so this one keeps its quotes when printed.
word_quoted(){ echo x >& "2"; }
# …and quoting the dash likewise makes it a word, not a close.
word_dash()  { echo x >& '-'; }
declare -f
echo "  a close is never printed with the input arrow: rc=$(declare -f | grep -c '<&-')"

echo "=== a dup word on fd 1 is the ampersand-first form wearing other clothes"
{ echo to-stdout; echo to-stderr >&2; } >& both; cat both
{ echo to-stdout; echo to-stderr >&2; } &> both2; cat both2
echo "  and appending has no dup spelling, so only &>> exists:"
{ echo more >&2; } &>> both2; cat both2

echo "=== the special filenames are dups either way round"
( echo via-word >& /dev/stderr ) 2> cap; cat cap
( echo via-amp  &> /dev/stderr ) 2> cap; cat cap
( echo with-fd  1>& /dev/stderr ) 2> cap; cat cap

echo "=== on any other descriptor there is no such rewrite"
echo x 3>& somefile; echo "  rc=$?"
echo x 2>& somefile; echo "  rc=$?"

echo "=== the decision waits for the expansion"
v=2
echo to-fd-2 >& $v 2> viafd; cat viafd
v=viaword
echo to-a-file >& $v; cat viaword

echo "=== exec takes the same route"
( exec >& e1; echo out; echo err >&2 ); cat e1
( exec 2> e2; exec >& /dev/stderr; echo hi ); cat e2
( exec 3>& e3 ) 2>&1; echo "  rc=$?"

echo "=== and a failed open is reported against the expansion"
echo x >& nodir/f; echo "  rc=$?"

echo "=== a backgrounded job carries the rewrite too"
# A `& ` job is a forked child, so its list is applied in the child — and inside
# a `$( … )` the sink it starts with is the substitution's pipe. `>& file` takes
# fd 1 *and* fd 2 off that pipe, so the substitution collects neither stream.
# (Every job here is drained before its file is read, so nothing races.)
x=$( { echo s; echo e >&2; } >& sink1 & wait ); echo "  word=[$x] file=[$(cat sink1)]"
x=$( { echo s; echo e >&2; } &> sink2 & wait ); echo "  amp=[$x] file=[$(cat sink2)]"
# …whereas a dup *into* fd 1 from fd 2 gives fd 1 away without touching fd 2.
x=$(echo t >&2 & wait) 2>/dev/null; echo "  tostderr=[$x]"
v=sink3
x=$( { echo s; echo e >&2; } >& $v & wait ); echo "  viavar=[$x] file=[$(cat sink3)]"
