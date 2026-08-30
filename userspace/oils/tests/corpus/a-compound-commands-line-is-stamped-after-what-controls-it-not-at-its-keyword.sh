# The line a compound command is *executed* at is not the line its keyword sits
# on. Four of them carry a line of their own, and every one is stamped after
# reading something:
#
#   * `case`, `for` and `select` take `word_lineno[word_top]`, which the lexer
#     assigns in `read_token_word` once the word *after* the keyword has been
#     read (parse.y:5352-5357) — so the number is where the controlling word
#     ends: the `case` subject, or the loop variable;
#   * `for (( … ))` takes `arith_for_lineno`, stamped in `parse_dparen` where
#     the `((` is read (parse.y:4469);
#   * `[[ … ]]` takes its *root* node's, and `make_cond_node` stamps each node
#     as it is built (make_cmd.c:463). A term is built the moment its last
#     token has been read, but `&&`/`||` are built after the right-hand term's
#     `cond_skip_newlines` has already fetched the token behind it — so those
#     land on whatever closes the expression, usually the `]]`. `!` builds no
#     node at all; it flips a flag on the one it was given.
#
# The executors then set the shell's one `line_number` from it before expanding
# anything (execute_cmd.c:2897, 3405, 3544, 4029) and put it back afterwards,
# so the number below is the one every diagnostic raised while the *header* is
# expanded carries — while the body keeps its own lines throughout.
#
# All of this is invisible until the construct spans lines, which is why every
# case here is written across several.

echo "=== a case is blamed where its subject ends, not where the keyword is"
( case "a
b" in
"y${nope?bad}") ;;
esac )
echo "rc=$?"

echo "=== a for and a select are blamed where the loop variable ends"
( for \
  x \
  in "${nope?bad}"
do :
done )
echo "for rc=$?"
( select \
  x \
  in "${nope?bad}"
do :
done )
echo "select rc=$?"

echo "=== an arithmetic for is blamed where its (( is, in all three sections"
( for \
(( i=${nope?bad};
i<1; i++ )); do :; done )
echo "init rc=$?"
( for \
(( i=0;
i<${nope?bad}; i++ )); do :; done )
echo "cond rc=$?"
( for \
(( i=0;
i<2;
i+=${nope?bad} )); do echo body; done )
echo "update rc=$?"

echo "=== a conditional term is blamed where the term ends"
( [[ ${nope?bad} == x
]] )
echo "rc=$?"
( [[ -n ${nope?bad}
]] )
echo "unary rc=$?"
( [[ ! ${nope?bad} == x
]] )
echo "negated rc=$?"

echo "=== but a && or || is blamed where the whole expression closes"
( [[ ${nope?bad} == x &&
y == y ]] )
echo "left rc=$?"
( [[ y == y &&
${nope?bad} == x
]] )
echo "right rc=$?"
( [[ ${nope?bad} == x && y == y &&
z == z
]] )
echo "chain rc=$?"

echo "=== and a group closes it early, parentheses and all"
( [[ ( ${nope?bad} == x && y == y )
]] )
echo "group rc=$?"
( [[ ! ( ${nope?bad} == x && y == y )
]] )
echo "negated group rc=$?"

echo "=== the body of each keeps its own lines"
for \
  x \
  in a
do
  echo "for body $LINENO"
done
for (( i=0; i<1; i++ )); do
  echo "arith body $LINENO"
done
case \
  b \
  in
b)
  echo "case body $LINENO"
  ;;
esac

echo "=== and the line is put back when the compound is done"
case \
  q \
  in
q) ;;
esac
echo "after case $LINENO"
