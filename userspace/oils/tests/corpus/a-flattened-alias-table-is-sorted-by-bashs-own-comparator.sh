# `alias -p` sorts, and the order it sorts into is not the byte order every
# other listing uses. bash flattens the alias hash table through
# `all_aliases()`, which hands the array to `sort_aliases`, and so to
# `qsort_alias_compare`:
#
#   if ((result = (*as1)->name[0] - (*as2)->name[0]) == 0)
#     result = strcmp ((*as1)->name, (*as2)->name);
#
# The leading test reads as an optimization — decide on the first byte, fall
# back to `strcmp` only when it cannot — but the two halves disagree about what
# a byte is worth. `name[0]` is a `char`, signed on every platform bash builds
# on, while `strcmp` compares as *unsigned char*. So a name beginning with a
# byte at or above `\x80` is a negative number to the shortcut and sorts ahead
# of every ASCII name, while two names sharing a first byte are ordered by the
# ordinary unsigned comparison from there on: `\xff` before `a`, but `ab`
# before `a\xff`.
#
# The same flattening is what `compgen` reads. The `alias` action is
# `all_aliases()` outright, and a command completion reaches for it too — the
# alias run at the head of `compgen -c` is sorted by the same comparator, and
# only the aliases are: the reserved words, builtins and `$PATH` names behind
# them each keep their own order.
#
# What is *not* sorted is `BASH_ALIASES`, which enumerates the hash table
# itself. That order is a property of the table's shape and is pinned by
# `the-alias-and-command-tables-are-bashs-own.sh`; the two views of one table
# disagree on purpose.
#
# The names below are deliberately awkward. An alias name may hold any byte
# that is not a shell metacharacter, `/`, or whitespace, which leaves the whole
# high half of the byte range free to be sorted wrongly — so the listings are
# passed through `cat -v` to keep the comparison legible.

echo "=== a listing is sorted, and a high byte leads"
alias a=1 q=4 '~q'=3 $'\xff'=5 $'\x80z'=6
alias | cat -v
echo "  rc=$?"

echo "=== the first byte decides; only a tie lets the rest of the name speak"
unalias -a
alias $'\xffa'=1 $'\xff\x01'=2 $'a\xff'=3 ab=4 $'\x01z'=5 Zz=6 $'\x7f'=7
alias | cat -v
# `-p` with operands prints the whole sorted table first and *then* answers the
# queries, in operand order — the sort belongs to the dump, not to the query.
alias -p $'\x7f' ab | cat -v
echo "  rc=$?"

echo "=== compgen reads that same flattened list"
compgen -A alias | cat -v
echo "  rc=$?"
compgen -a | cat -v
echo "  rc=$?"

echo "=== …including the alias run at the head of a command completion"
unalias -a
alias zqqa=1 zqqb=2 zqqc=3 zqqd=4
# Nothing on `$PATH` answers to `zqq`, so what comes back is the alias run
# alone — and these four sit in the table in an order that is not their sorted
# one, so the two views can be told apart.
echo "  table -> ${!BASH_ALIASES[@]}"
compgen -c zqq
echo "  rc=$?"
compgen -A command zqq
echo "  rc=$?"

echo "=== but the mirror is the table, and the table is not sorted"
unalias -a
alias aft=: aoo=: atj=: bfs=:
echo "  sorted -> $(compgen -A alias | tr '\n' ' ')"
echo "  table  -> ${!BASH_ALIASES[@]}"

echo "=== and a name that no longer exists leaves the order it was in"
unalias -a
alias zz=1 aa=2 mm=3 bb=4
alias | cat -v
unalias mm
alias | cat -v
alias mm=9
compgen -A alias | cat -v
echo "  rc=$?"
