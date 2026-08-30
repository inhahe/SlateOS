# A `"` in text no parser read is expanded by `string_extract_double_quoted`,
# which is handed a **finished word** rather than a stream: its walk ends at the
# end of the string as readily as at a closing `"`. So an unmated `"` there is
# not a failure at all — it opens a run that takes the rest of the text, and
# quote removal drops the `"` alone.
#
# Reaching that shape takes some doing, because the `${ … }` scan normally gets
# to the `"` first. `extract_dollar_brace_string` meets it, hands the rest of
# the word to a double-quote skipper that never comes back, and raises
# `bad substitution` before there is an operand to expand — which is why
# `${nope:-a"b}c` and `${nope#a"b}c` are both condemned below rather than
# expanded.
#
# The exception is a `' … '` run, which that scan steps over **whole**
# (`skip_single_quoted`, subst.c:1926-1938). A `"` inside one is invisible to
# it, so the brace closes normally and the operand is handed on still carrying
# the unmated quote. `${nope:-'a"b'}` is that: to the *expansion* the `'`s are
# ordinary characters — `expand_word_internal` is long past the point where a
# `'` could quote anything — so what it sees is `'a`, then a run holding `b'`,
# then the end of the operand. The answer is `'ab'`.
#
# The same word with the substitution's variable *set* never expands the operand
# at all, and bash is quiet: `${z:-'a"b'}` is just `ZZ`. That is the row worth
# guarding, because a shell that lexes the whole word up front meets the unmated
# `"` whether the operand is used or not.
#
# Verified against bash 5.2.37.

z=ZZ
unset nope

echo "=== the plain shapes: the brace scan meets the quote itself ==="
v='A${nope:-a"b}c'; printf '[%s]\n' "${v@P}"
v='A${nope#a"b}c'; printf '[%s]\n' "${v@P}"
v='A${nope/a/b"c}d'; printf '[%s]\n' "${v@P}"

echo "=== a run with a mate is unremarkable ==="
v='A${nope:-a"b"c}d'; printf '[%s]\n' "${v@P}"

echo "=== an unmated quote inside a single-quoted run: the operand is used ==="
v='A${nope:-'"'"'a"b'"'"'}c'; printf '[%s]\n' "${v@P}"

echo "=== …and the same word with the operand unused ==="
v='A${z:-'"'"'a"b'"'"'}c'; printf '[%s]\n' "${v@P}"

echo "=== the run really does take the rest of the operand ==="
v='A${nope:-'"'"'a"b'"'"'c}d'; printf '[%s]\n' "${v@P}"
v='A${nope:-x'"'"'p"q'"'"'y}z'; printf '[%s]\n' "${v@P}"

echo "=== two runs, each with an unmated quote ==="
v='A${z:-'"'"'a"b'"'"''"'"'c"d'"'"'}e'; printf '[%s]\n' "${v@P}"

echo "=== an even number of quotes in the run closes on its own ==="
v='A${z:-'"'"'a"b"c'"'"'}d'; printf '[%s]\n' "${v@P}"

echo "=== the quote stays literal at the top level of the text ==="
v='A'"'"'a"b'"'"'c'; printf '[%s]\n' "${v@P}"

echo "=== a live substitution inside the unmated run still expands ==="
v='A${nope:-'"'"'p"$(echo hi)q'"'"'}z'; printf '[%s]\n' "${v@P}"

# The run takes the newline with everything else — there is nothing about the
# end of a line that ends it, only the end of the operand.
echo "=== …across a newline, with the rest of the operand behind it ==="
v='A${nope:-'"'"'i"t'"'"'$(echo hi)
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== and one that will not close is still read, and still condemns ==="
v='A${nope:-a"$(echo hi}c'; printf '[%s]\n' "${v@P}"

# `${x@P}` is only one of the ways text reaches an expansion unread. A
# here-document body is another, and it is the one that matters most: the
# operand is not a value the script chose to expand there but ordinary source,
# so a shell that calls the unmated `"` an error does not merely print the wrong
# thing — it fails to read the script at all.
echo "=== the same operand in a here-document body ==="
cat <<E
A${nope:-'a"b'}c
A${z:-'a"b'}c
E

echo TAIL
