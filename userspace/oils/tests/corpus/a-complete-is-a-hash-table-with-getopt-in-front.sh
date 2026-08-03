# `complete` looks like a registry keyed by command name, and it is — but two
# things about it are decided elsewhere, and both leak.
#
# The first is that the registry is a *hash table*, so a bare `complete -p`
# dumps it in the table's own order: bucket ascending, and within a bucket
# newest-first, because an insert links at the head of the chain. Redefining a
# name reuses the item already in the chain, so it keeps its place. The three
# specials live in the same table under reserved names no command word could
# be, and the diagnostics never translate them back.
#
# The second is that the option scan in front of it is plain getopt, so the
# first word that is not an option ends the options — which makes a flag
# written after a name into a name, and makes `-D`/`-E`/`-I` behave less like
# targets and more like a mode that discards the operands entirely.

echo "=== the print order is the table's, not the script's"
# Four names in one order and then the reverse: if the listing followed
# insertion, these two blocks would be reverses of each other. They are not —
# the second is the *same* order as bash's buckets happen to give.
complete -W 1 aaa; complete -W 1 bbb; complete -W 1 ccc; complete -W 1 ddd
complete -p
complete -r
complete -W 1 ddd; complete -W 1 ccc; complete -W 1 bbb; complete -W 1 aaa
complete -p
complete -r
# Enough names to spread over many buckets, so the ordering is not a
# coincidence of four.
for i in $(seq 1 40); do complete -W 1 "n$i"; done
complete -p | sed 's/^complete -W .1. //' | tr '\n' ' '; echo
complete -r

echo "=== two names in one bucket come out newest-first"
# c8 and c174 collide. Both insertion orders, so the tie-break is visible on
# its own rather than mixed with the bucket ordering.
complete -W 1 c8; complete -W 1 c174
complete -p | sed 's/^complete -W .1. //' | tr '\n' ' '; echo
complete -r
complete -W 1 c174; complete -W 1 c8
complete -p | sed 's/^complete -W .1. //' | tr '\n' ' '; echo
complete -r
# Redefining reuses the item, so it does not move to the head.
complete -W 1 c8; complete -W 1 c174; complete -W 2 c8
complete -p | tr '\n' ' '; echo
complete -r

echo "=== the specials share the table, and the names show"
complete -p -D; echo "  D rc=$?"
complete -p -E; echo "  E rc=$?"
complete -p -I; echo "  I rc=$?"
complete -r -D; echo "  rD rc=$?"
# And they are ordered among the ordinary names by the same hash.
complete -W 1 -D; complete -W 1 -E; complete -W 1 -I; complete -W 1 c8
complete -p | tr '\n' ' '; echo
complete -r; complete -r -D; complete -r -E; complete -r -I

echo "=== only the first special counts, in a fixed order"
# Written -I -E -D, but -D is the one defined, and the other two stay missing.
complete -W 1 -I -E -D; echo "  rc=$?"
complete -p
complete -r -D
complete -W 2 -I -E; echo "  rc=$?"
complete -p
complete -r -E
# A special also replaces the operands rather than joining them: kk is never
# mentioned again.
complete -W 3 -D kk; echo "  rc=$?"
complete -p; echo "  p rc=$?"
complete -r 2>/dev/null; complete -r -D 2>/dev/null
# The same holds for -p and -r, which read the operand list the same way.
complete -W 1 zz; complete -W 1 -D
complete -p -D zz; echo "  p rc=$?"
complete -r -D zz; echo "  r rc=$?"
complete -p; echo "  left rc=$?"
complete -r

echo "=== options stop at the first name"
# -D here is an operand, so a spec named -D is what gets defined.
complete -W 1 foo -D; echo "  rc=$?"
complete -p foo; echo "  foo rc=$?"
complete -p -D; echo "  D rc=$?"
complete -p -- -D; echo "  litD rc=$?"
complete -r; complete -r -D 2>/dev/null
# Three specs, and none of them carries the nospace option.
complete -W 1 aa -o nospace; echo "  rc=$?"
complete -p aa; complete -p -- -o; complete -p nospace
complete -r

echo "=== -r answers for each name it was given"
complete -r zzz; echo "  rc=$?"
complete -W 1 have
complete -r m1 have m2; echo "  rc=$?"
complete -p have; echo "  have rc=$?"
# Removing the whole table succeeds whether or not there was anything in it.
complete -r; echo "  all rc=$?"
complete -r; echo "  again rc=$?"

echo "=== a printed name is quoted only for a shell metacharacter"
for n in 'a b' "a'b" 'a"b' 'a\b' 'a|b' 'a&b' 'a;b' 'a(b' 'a<b' 'a>b' 'a!b' 'a{b' 'a*b' 'a[b' 'a?b' 'a^b' 'a$b' 'a`b' 'a~b' '~ab' 'a#b' '#ab' 'a-b' 'a_b' 'a=b' 'a:b' 'a,b' 'a.b' 'a/b' 'a+b' 'a%b' 'a@b'; do
  complete -W 1 "$n"
  complete -p "$n" | sed 's/^complete -W .1. //'
  complete -r "$n"
done
complete -W 1 ''; complete -p ''; echo "  rc=$?"
complete -r ''

echo "=== -F is checked against the word-breaking characters, not identifiers"
# So a leading digit is fine and an empty argument is fine, but anything that
# would end a word is not — and the check happens where it is written, before
# a later option's own complaint.
t() { complete -F "$1" nm 2>&1; echo "  [$1] rc=$?"; complete -r nm 2>/dev/null; }
t 'ok_name'
t '1bad'
t ''
t 'f-x'
t 'f.x'
t 'f$x'
t 'f/x'
t 'f x'
t 'f;x'
t 'f&x'
t 'f|x'
t 'f(x'
t 'f<x'
complete -A nosuch -F 'f x' nm; echo "  action-first rc=$?"
complete -F 'f x' -A nosuch nm; echo "  F-first rc=$?"
complete -F 'f x'; echo "  no-name rc=$?"
# The same word is unremarkable everywhere else.
complete -W 'f x' n2; complete -p n2
complete -C 'f x' n3; complete -p n3
complete -G 'f x' n4; complete -p n4
complete -r

echo "=== the table is forked, not dropped, by a subshell"
complete -W 1 sub
( complete -p ) ; echo "  paren rc=$?"
complete -p | cat; echo "  pipe rc=$?"
# ...and a child's change does not come back.
( complete -W 2 sub2 ); complete -p sub2; echo "  child rc=$?"
complete -r
