# `compgen` as a *question about order*: what it can offer is rarely in doubt,
# but the sequence it offers things in is decided in three separate places —
# how the options were read, which source each candidate came from, and what
# that source's own idea of order is — and none of those is the order the
# options were written in. The status is a fourth answer again: asking for
# nothing is not the same as asking and being given nothing.
#
# Everything here names its own candidates or its own directory. A bare dump of
# a shell table is avoided wherever the two shells have no reason to hold the
# same entries: `-c` reaches $PATH, `-b`/helptopic reach a builtin set, `-v`
# reaches the shell's identity variables, `-A signal` reaches the host's signal
# numbering. Where a table *is* dumped it is one both shells build from the
# language itself.

echo "=== an argument-taking letter swallows the rest of its cluster"
compgen -Wab
compgen -W'a b'
# …and a letter that takes none can share the cluster with it.
alias aw=x
compgen -aW 'x y'
f1() { :; }
compgen -Afunction f
# The scan stops at the first operand, so a word written before an option hides
# it: nothing was asked for, and nothing is reported.
compgen zzz -W 'a b'; echo "  hidden rc=$?"
# `--` ends the options; the first operand after it is the word, and bash never
# looks past it.
compgen -W 'aa ab b' -- a; echo "  word rc=$?"

echo "=== asking for nothing succeeds; asking and getting nothing does not"
compgen; echo "  bare rc=$?"
compgen --; echo "  dashdash rc=$?"
compgen -W ''; echo "  empty-W rc=$?"
compgen -P ''; echo "  bare-P rc=$?"

echo "=== the two names checked against a table are checked as they are read"
# So the complaint arrives before anything a valid option would have generated,
# and the status is the usage error's 2 rather than the empty answer's 1.
compgen -A nosuch x; echo "  action rc=$?"
compgen -o nosuchopt; echo "  option rc=$?"
compgen -W 'a b' -A nosuch; echo "  early rc=$?"
compgen -W; echo "  missing rc=$?"

echo "=== the sources are emitted in bash's order, not the options' order"
# The actions are a bitmask, so each generates exactly once and they run in the
# order of the action table — which is why these two agree.
declare -x xvar1=1
compgen -e -A alias xv
compgen -A alias -e xv
# The wordlist is appended after every action…
compgen -W 'if1' -k i
# …and the two filesystem actions are the last entries in that table, so `-d -f`
# answers files first and directories second — with the wordlist behind both,
# because it is not an action at all. (`-G` sits between the two: actions,
# then the glob, then the wordlist, whatever order the options were written in.)
mkdir gd; touch g1
compgen -d -f -W 'gw' g

echo "=== a repeated option overwrites, except for the actions, which gather"
# Each of these is one slot in the compspec.
compgen -W 'x y' -W 'p q'; echo "  W rc=$?"
compgen -P '<' -P '[' -W a; echo "  P rc=$?"
# The actions are the exception: they are a set, and a repeat costs nothing.
compgen -A alias -A alias aw; echo "  A rc=$?"

echo "=== -G is expanded as a pathname and offered whole"
touch ga1 gb1
# The word does not narrow it — the pattern already said what to match — and the
# expansion comes back in the reverse of the order the *directory was read*, not
# of the order the pattern would have expanded to as a word. bash reaches the
# glob matcher directly here, so the sort that pathname expansion applies never
# runs, and what is left is the matcher's list, built by prepending each name as
# it comes off readdir. In a directory whose `ls -f` reads
# `gc1 gm1 gk1 gb1 gq1 gd1 gz1 ga1`, `compgen -G 'g*1'` answers
# `ga1 gz1 gd1 gq1 gb1 gk1 gm1 gc1` where `echo g*1` answers the sorted list.
# Both shells read the same directory here, so the two agree; this file said
# only "reversed" until 2026-08-26, which was true of the sorted order it was
# measured against on MSYS and is not true of a filesystem that hashes names.
compgen -G 'g*1'
compgen -G 'g*1' zzz
# But -X and -P/-S still reach it, because those are about the answer.
compgen -G 'g*1' -X 'gb*'
compgen -G 'g*1' -P '<' -S '>'
# One slot again: the second pattern replaces the first.
compgen -G 'ga*' -G 'gb*'
compgen -G 'nosuch*'; echo "  nomatch rc=$?"

echo "=== the -o fallbacks stand outside the compspec"
# They are readline's own completions rather than compspec sources, so neither
# the filter nor the decoration reaches them. `plusdirs` adds directories to
# whatever was found, however much that was…
compgen -o plusdirs -W 'gw' -P '<' -S '>' g
compgen -o plusdirs -W 'gw' -X 'g*' g
# …and it counts, so an answer it filled is not an empty one.
compgen -o default -o plusdirs g; echo "  plus rc=$?"
# The other two step in only for an answer that came out empty — even one the
# filter emptied — and directories win over filenames.
compgen -o default g
compgen -o dirnames g
compgen -W 'g1' -o default -X '*' g
compgen -o default -o dirnames g
compgen -o default -o plusdirs zz; echo "  none rc=$?"
# An -o that says nothing about candidates still asks for a compspec, so it
# gets the empty answer rather than the silent success.
compgen -o nosort g; echo "  nosort rc=$?"

echo "=== a leading dot is an ordinary character to path completion"
mkdir pd && cd pd
mkdir adir && touch a1 .ahid
# A hidden name is offered to an empty word like any other; only `.` and `..`
# are held back from one, because naming the directory itself would be no help.
# The order is the directory's own — this is a bare readdir loop, so it is `ls
# -f` order and not a sorted one.
compgen -f ''
compgen -d ''
# A word that is a dot reaches all three. Sorted, and only here, because to bash
# `.` and `..` are two readdir entries like any other and so land wherever the
# filesystem put them (measured: `.. .mhid .bhid .zhid .ahid .`), while Rust's
# directory reader drops both on every platform and leaves osh to name them from
# outside the loop. The set is the fact; the position is the filesystem's.
compgen -f . | sort
compgen -d . | sort
compgen -f a
# The rule follows the word's own directory component, not the cwd.
compgen -f adir/. | sort
cd ..

echo "=== the tables both shells build from the language itself"
# Reserved words come out in the grammar's order, not sorted.
compgen -k
# `set -o` names are already alphabetical (igncr is a Cygwin-only addition).
compgen -A setopt | grep -v '^igncr$'
# `shopt` names come out in the shopt table's order.
compgen -A shopt | head -6
# A builtin is enabled until it is not, and then it is on the other list.
compgen -A enabled tr
enable -n true
compgen -A enabled tr
compgen -A disabled tr

echo "=== names the script itself made"
zarr=(1 2); declare -A zmap=([k]=v); zplain=1
compgen -A arrayvar z
compgen -A variable z
declare -x zx1=1
compgen -A export z
zf1() { :; }
zf2() { :; }
compgen -A function zf
