# `declare w=a{1,2}` is two words, not one. A declaration builtin's operand is
# still a *word*, and brace expansion runs over the whole word list before
# anything else — the `NAME=` prefix included. `brace_expand_word_list`
# (subst.c:12427-12439) excuses only two kinds of word:
#
#     if (tlist->word->flags & W_NOBRACE) …continue;
#     if ((tlist->word->flags & (W_COMPASSIGN|W_ASSIGNARG)) ==
#           (W_COMPASSIGN|W_ASSIGNARG)) …continue;
#
# — and a *scalar* assignment operand is only `W_ASSIGNARG`, so it expands. A
# *compound* one is both flags and is left alone, which is why a brace inside a
# `( … )` key survives while its elements, being words in their own right, do
# not.
#
# The several words that come out are then each performed as an assignment, so
# the visible result is the last one's — except where performing one has an
# effect of its own: `readonly r=z{1,2}` assigns `z1`, marks `r` read-only, and
# then *fails* on `z2`.
#
# Verified against bash 5.2.37.

echo "=== a scalar operand expands, and the last assignment wins ==="
declare w=a{1,2}
declare -p w
readonly r=z{1,2}
declare -p r
export e=v{1,2}
declare -p e
f() { local l=m{1,2}; declare -p l; }
f

echo "=== …and each word is performed in turn, so an append appends twice ==="
declare c+=q{1,2}
declare -p c
declare -i n=1{0,1}
declare -p n

echo "=== a compound operand is not expanded, but its elements are ==="
declare -A m=([k{1,2}]=v)
declare -p m
declare -a A=(x{1,2} y)
declare -p A

echo "=== the rest of the word expands with it ==="
declare g=a{1,2}=b
declare -p g
declare h=/dev/nul{l,l}
declare -p h
declare u=~{1,2}
declare -p u

echo "=== and what brace expansion never touches is untouched here too ==="
declare q="{1,2}"
declare -p q
declare s='a{1,2}'
declare -p s
set +B
declare t=b{1,2}
declare -p t
set -B
echo TAIL
