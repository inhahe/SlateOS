# The fragments beside a `${ … }` operand — the pattern of `#`/`%`/`^`/`,`, the
# two halves of `/pat/repl`, and the bounds of `:off:len` — are read with the
# *same read* as the operand, and with a different *quoting*. The two questions
# are independent, and bash answers them in two different places.
#
# The quoting: a pattern is expanded by `getpattern`, which throws the enclosing
# context away before it starts —
#
#     pat = expand_string_for_pat (value,
#             (quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) ? Q_PATQUOTE : quoted,
#             (int *)NULL, (int *)NULL);          /* subst.c:5751-5754 */
#
# — so in text that expands in `Q_DOUBLE_QUOTES` as a whole (a here-document
# body with an unquoted delimiter) an operand's `$'…'` stays as written while a
# pattern's is translated. The same split holds one level down: an operand
# *inside a pattern* has inherited the pattern's `Q_PATQUOTE`.
#
# The read: nothing gives a pattern back a parse the text never had. A
# here-document body goes from `read_secondary_line` straight into
# `make_here_document` (make_cmd.c:621), so no `read_token_word` ever ran over
# it — and a `$( … )` in a pattern there is read at expansion time, by
# `extract_command_subst`, exactly like one in the operand beside it. When that
# read fails it is the brace's own scan that fails, which is the whole of
# `a-failed-extent-parse-inside-a-brace-operand-does-not-kill-the-brace.sh`.
#
# Verified against bash 5.2.37.

y=Y

echo "=== the scan reads a pattern's extent, and names the whole text"
a='A${y#$(fi)}B';     printf '1 [%s]\n' "${a@P}"
b='A${y%%$(fi)}B';    printf '2 [%s]\n' "${b@P}"
c='A${y^$(fi)}B';     printf '3 [%s]\n' "${c@P}"
d='A${y,,$(fi)}B';    printf '4 [%s]\n' "${d@P}"

echo "=== both halves of a replacement, and both bounds of a slice"
e='A${y/$(fi)/z}B';   printf '5 [%s]\n' "${e@P}"
f='A${y/Y/$(fi)}B';   printf '6 [%s]\n' "${f@P}"
g='A${y//$(fi)/z}B';  printf '7 [%s]\n' "${g@P}"
h='A${y:$(fi):1}B';   printf '8 [%s]\n' "${h@P}"
i='A${y:0:$(fi)}B';   printf '9 [%s]\n' "${i@P}"

echo "=== a pattern nested in an operand, and an operand nested in a pattern"
j='A${y:-p${y#$(fi)}q}B'; printf '10 [%s]\n' "${j@P}"
k='A${y#${y:-$(fi)}}B';   printf '11 [%s]\n' "${k@P}"

echo "=== a here-document body is the same unread text, and survives it too"
( cat <<E
A${y#$(fi)}B
E
) 2>&1
echo "after=$?"
( cat <<E
A${y/Y/$(fi)}B
E
) 2>&1
echo "after=$?"
( cat <<E
A${y:$(fi):1}B
E
) 2>&1
echo "after=$?"

echo "=== the quoting is not the operand's: getpattern drops it"
v=$'a\tb'
w='a\tb'
cat <<E
12 [${nope:-$'a\tb'}]
13 [${v#$'a\tb'}]
14 [${w#$'a\tb'}]
15 [${v#${z:-$'a\tb'}}]
16 [${w#${z:-$'a\tb'}}]
17 [${v/$'a\tb'/X}]
E

# The scan only ever *parses* a `$( … )`; a backquote goes to `string_extract`
# and a `$((` to `extract_delimited_string`, neither of which reads its own
# extent. So neither of these two is a bad substitution — each fails later, on
# its own terms, with the brace still standing.
echo "=== a backquote and a \$(( in a pattern are only stepped over"
m='A${y#`fi`}B';       printf '18 [%s]\n' "${m@P}"
n='A${y#$((1+))}B';    printf '19 [%s]\n' "${n@P}"

echo done
