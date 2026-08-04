# `BASH_ALIASES` and `BASH_CMDS` look like associative arrays and are not. They
# are views of two *other* hash tables — bash makes a separate `hash_create` for
# aliases and for hashed commands — and those tables have their own sizes, so
# they enumerate in their own orders. Filling an ordinary associative array with
# the same keys gives a third order again, which is what osh used to print.
#
# Everything else is shared with `an-associative-array-iterates-in-bashs-hash-order.sh`:
# the same FNV-1 over signed chars, the same `hash & (nbuckets - 1)`, head
# insertion, and a growth that multiplies the bucket count by four and reverses
# each chain on the way. Only the initial size differs — 64 for aliases, 256 for
# hashed commands, 1024 for an associative array — and that is enough to change
# every bucket index and therefore the whole order.
#
# `aft aoo atj bfs` share a bucket at 64 buckets *and* at 256, so they are the
# set that shows chain order rather than bucket order. Sequential names like
# `al0 al1 …` do not collide at all, which is why they can say nothing about it.
#
# The two readers of a table disagree on purpose: `hash`/`hash -l` walk each
# chain from the head, the `BASH_CMDS` mirror comes out the other way, and
# `alias -p` ignores the table order entirely and sorts.

echo "### the alias table, four names in one bucket"
a4() { unalias -a; for n; do alias "$n=:"; done; echo "  [$*] -> ${!BASH_ALIASES[@]}"; }
a4 aft aoo atj bfs
a4 bfs atj aoo aft
a4 atj bfs aft aoo

echo "### a removal unlinks, and a re-add goes to the head"
unalias -a; alias aft=: aoo=: atj=: bfs=:
echo "  all      -> ${!BASH_ALIASES[@]}"
unalias atj
echo "  -atj     -> ${!BASH_ALIASES[@]}"
alias atj=:
echo "  +atj     -> ${!BASH_ALIASES[@]}"
alias aft=x
echo "  rewrite  -> ${!BASH_ALIASES[@]}"
echo "  values   -> ${BASH_ALIASES[@]}"

echo "### alias -p sorts; the mirror does not"
alias -p
declare -p BASH_ALIASES

echo "### unalias -a throws the table away"
unalias -a
echo "  n=${#BASH_ALIASES[@]}"
declare -p BASH_ALIASES
alias zz=: aa=: mm=: bb=:
echo "  again    -> ${!BASH_ALIASES[@]}"

echo "### forty aliases, which collide with nothing"
unalias -a
for i in 0 1 2 3 4 5 6 7 8 9; do alias "al$i=:" "al1$i=:" "al2$i=:" "al3$i=:"; done
echo "  n=${#BASH_ALIASES[@]}"
echo "  ${!BASH_ALIASES[@]}"

echo "### and past the growth at 128, where a chain reverses"
unalias -a
alias aft=: aoo=: atj=: bfs=:
i=0; while [ $i -lt 130 ]; do alias "z$i=:"; i=$((i+1)); done
echo "  n=${#BASH_ALIASES[@]}"
set -- ${!BASH_ALIASES[@]}
for k; do case $k in aft|aoo|atj|bfs) printf ' %s' "$k";; esac; done; echo

echo "### the hashed-command table is a different size again"
c4() { hash -r; for n; do hash -p "/p/$n" "$n"; done; echo "  [$*] -> ${!BASH_CMDS[@]}"; }
c4 aft aoo atj bfs
c4 bfs atj aoo aft

echo "### hash walks the chain the other way from the mirror"
hash -r; hash -p /p/aft aft; hash -p /p/aoo aoo; hash -p /p/atj atj; hash -p /p/bfs bfs
hash
hash -l
echo "  mirror -> ${!BASH_CMDS[@]}"
echo "  values -> ${BASH_CMDS[@]}"
declare -p BASH_CMDS

echo "### hash -r empties it, and -d unlinks one"
hash -r
echo "  n=${#BASH_CMDS[@]}"
hash -p /p/aft aft; hash -p /p/aoo aoo; hash -p /p/atj atj
echo "  three  -> ${!BASH_CMDS[@]}"
hash -d aoo
echo "  -aoo   -> ${!BASH_CMDS[@]}"
hash -p /p/aoo aoo
echo "  +aoo   -> ${!BASH_CMDS[@]}"

echo "### the same keys in an ordinary associative array: a third order"
declare -A m=([aft]=1 [aoo]=1 [atj]=1 [bfs]=1)
echo "  assoc  -> ${!m[@]}"

echo "### a subshell inherits both tables as they stand"
unalias -a; hash -r
alias aft=: aoo=: atj=: bfs=:
hash -p /p/aft aft; hash -p /p/aoo aoo
( echo "  sub-al -> ${!BASH_ALIASES[@]}"; echo "  sub-cm -> ${!BASH_CMDS[@]}" )

echo "### writing through the mirror defines through the table"
unalias -a; hash -r
BASH_ALIASES[aft]=:; BASH_ALIASES[aoo]=:; BASH_ALIASES[atj]=:
echo "  written -> ${!BASH_ALIASES[@]}"
alias -p
