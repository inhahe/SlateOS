# `readonly q=(1 2)` is not two commands that each own a kind of operand — it is
# one builtin call whose operand list happens to have been rewritten.
#
# bash decomposes an assignment word of a declaration builtin into three commands
# (`expand_declaration_argument`, subst.c:12653): a `declare` over the word
# **truncated at the `=`** (subst.c:12493), the compound assignment proper, and
# then the same truncated word handed to the **real builtin** once the whole
# word-expansion pass is over. A *scalar* operand never enters that
# decomposition at all — it is reached only for `W_COMPASSIGN` words
# (subst.c:12815) — so it arrives at that third command whole. Compound and
# scalar operands therefore meet in one argument list, each at the position it
# was written, and the builtin's own getopt scan runs over the command's flags
# exactly once whether or not any operand survived as a `name=value` string.
#
# Everything below is a consequence of that single scan, and none of it is
# reachable by a command whose operands are all compound if the marking is done
# by some other loop: the flags are only ever looked at by the builtin.
#
#   * an option letter neither builtin takes is refused, with the usage synopsis
#     and status 2 — and nothing is marked, though the literal has long since
#     bound (the assignment is command two; the refusal is command three);
#   * `-f` names the *function* namespace, so a variable operand is `not a
#     function` and the command fails 1;
#   * `-n` is not the nameref letter here: `readonly -n` declares without
#     marking, `export -n` unmarks — and each builtin reads the letter for
#     itself;
#   * `-p` given names is not a listing: it marks them and prints nothing;
#   * a word these two do not take as an option is an *operand*, `+a` and a
#     trailing `-x` alike, and only that operand is lost.
#
# The `-i` row is the one that reaches both halves. It is not a `readonly`
# option, so command three refuses it — but it *is* one of the value-transforming
# letters `make_internal_declare` copies into command one (subst.c:12493), so the
# array is integer-typed by the time the refusal is spoken.

echo '=== an option letter the builtin does not take'
for c in 'readonly -Z g=(1 2)' 'readonly -x g=(1 2)' 'readonly -i g=(1 2)' \
         'readonly -aZ g=(1 2)' 'export -q g=(1 2)' 'export -r g=(1 2)'; do
  ( eval "$c"; echo "rc=$?"; declare -p g ) 2>&1
done

echo '=== -f is the function namespace'
for c in 'readonly -f g=(1 2)' 'export -f g=(1 2)' 'readonly -f g=(1 2) h=(3)'; do
  ( eval "$c"; echo "rc=$?"; declare -p g h ) 2>&1
done

echo '=== -n, which neither reads as the nameref letter'
for c in 'readonly -n g=(1 2)' 'export -n g=(1 2)' 'readonly -na g=(1 2)' \
         'export -na g=(1 2)' 'readonly -n g=(1 2) s=5' 'export -n g=(1 2) s=5'; do
  ( eval "$c"; echo "rc=$?"; declare -p g s ) 2>&1
done

echo '=== -p given names marks them and prints nothing'
for c in 'readonly -p g=(1 2)' 'export -p g=(1 2)' 'readonly -pa g=(1 2)'; do
  ( eval "$c"; echo "rc=$?"; declare -p g ) 2>&1
done

echo '=== -- ends the options, and a non-option word is an operand'
for c in 'readonly -- g=(1 2)' 'readonly +a g=(1 2)' 'export +x g=(1 2)' \
         'readonly g=(1 2) -x' 'export -A g=([k]=v) -q'; do
  ( eval "$c"; echo "rc=$?"; declare -p g ) 2>&1
done

echo '=== the same scan, with a scalar operand in the list to prove it is one call'
for c in 'readonly -Z g=(1 2) s=5' 'readonly -f g=(1 2) s=5' 'readonly g=(1 2) s=5' \
         'export g=(1 2) s=5' 'readonly s=5 g=(1 2)'; do
  ( eval "$c"; echo "rc=$?"; declare -p g s ) 2>&1
done

echo '=== and inside a function, where the value goes global and the mark follows'
for c in 'readonly -Z g=(1 2)' 'readonly -f g=(1 2)' 'readonly g=(1 2)' 'export -n g=(1 2)'; do
  ( f() { eval "$c"; echo "rc=$?"; }; f; declare -p g ) 2>&1
done
