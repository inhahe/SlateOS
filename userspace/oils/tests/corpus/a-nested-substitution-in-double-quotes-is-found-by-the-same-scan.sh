# Finding where a `$( … )` ends means finding its `)`, and a double-quoted span
# inside it is not opaque to that search: substitution still happens in there, so
# a `)` in the span may be a *nested* substitution's rather than text, and a `<<`
# in there may declare a here-document whose body has to be fetched from past the
# enclosing `)`. bash's `parse_matched_pair` recurses into `$( … )`, `${ … }`,
# `$(( … ))` and a backtick body found inside the quotes, with the same reader.
#
# The two halves of that, each with cases below:
#
#   * a here-document declared inside a quoted nested substitution is gathered
#     like any other — at the nested `)`, from the lines after the enclosing
#     line, in the reader's order (see
#     `a-here-document-past-two-closes-is-fetched-at-the-inner-one`);
#   * and everything that merely *looks* like a paren in a quoted span is still
#     text: a bare `)`, a `)` inside a `${…}` default, a `))` closing an
#     arithmetic expansion, a `(` after a positional parameter.
#
# The one thing bash does not recurse into is a **backtick** body: its
# here-document is that lex's business, and bash never fetches it — so
# `` "`cat <<B`" `` inside a substitution leaves the body lines to run as
# commands. That is pinned below too, because it is the asymmetry, not an
# oversight to be smoothed over.
#
# See known-issues TD-OILS-CMDSUB-HEREDOC-INSIDE-A-QUOTED-NESTED-SUBSTITUTION.

echo "=== a here-document inside a quoted nested substitution is gathered"
x=$( echo "$(cat <<B)" ); echo "x=[$x]"
bbb
B

echo "=== …with the rest of the quoted string around it"
x=$( echo "pre $(cat <<B) post" ); echo "x=[$x]"
bbb
B

echo "=== the quoted one is inner, so it takes the first lines and warns first"
x=$(cat <<A; echo "$(cat <<B)" mid); echo "x=[$x]"
bbb
B
aaa
A

echo "=== a body delimited in place inside the quotes leaves nothing to fetch"
x=$( echo "$(cat <<B
bbb
B
)" ); echo "x=[$x]"

echo "=== two quoted levels are still one reader"
x=$( echo "$(echo "$(cat <<C)")" ); echo "x=[$x]"
ccc
C

echo "=== a paren in a quoted span that closes nothing"
x=$( echo "a)b" ); echo "x=[$x]"
x=$( echo "${v:-)}" ); echo "x=[$x]"
x=$( echo "cost $5(x)" ); echo "x=[$x]"
x=$( echo "$(echo "in)ner")" ); echo "x=[$x]"

echo "=== an arithmetic expansion in the quotes, where << is a shift"
x=$( echo "$((1 << 2))" ); echo "x=[$x]"
x=$( echo "$(( (1+2) * 3 ))" ); echo "x=[$x]"

echo "=== a backtick body is the exception: bash does not fetch its here-doc"
x=$( echo "`cat <<B`" ); echo "x=[$x]"
bbb
B
echo "after=$?"

echo done
