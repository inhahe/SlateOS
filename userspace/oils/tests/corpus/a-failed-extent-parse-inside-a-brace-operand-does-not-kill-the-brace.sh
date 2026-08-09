# A `${ … }` is *scanned* before any of it is expanded, and the scan reads the
# extent of a `$( … )` between the braces with a real parse:
#
#     if (string[i] == '$' && string[i+1] == LPAREN)
#       { si = i + 2; t = extract_command_subst (string, &si, flags|SX_NOALLOC); }
#                                                    /* subst.c:1896-1902 */
#
# Two things follow from that being a *scan*:
#
#   * it runs whether or not the operand is ever used — `y=Y; ${y:-p$(fi)q}`
#     reports even though the `:-` word is not substituted;
#   * when the read fails with its jump suppressed (a prompt expansion) it
#     leaves `si` on the last byte of the whole string, so `i = si + 1` walks
#     the scan off the end with `nesting_level` still 1.
#     `extract_dollar_brace_string` returns NULL, and `parameter_brace_expand`
#     makes that its own diagnostic naming the whole text:
#
#         value = extract_dollar_brace_string (string, &sindex, quoted, …);
#         if (string[sindex] == RBRACE) sindex++;
#         else goto bad_substitution;              /* subst.c:9913-9918 */
#
# So the operand never expands and the failed body is never *run* — unlike the
# same failure at the string level, which runs the remainder less a byte.
#
# Only a `$( … )` spelling can fail this way: a backquote is stepped over by
# `string_extract` and a `$((` goes to `extract_delimited_string`, and neither
# parses its own extent. A `' … '` run is stepped over too — which is the scan's
# rule and not the expansion's, so the two disagree in plain sight. A `[ … ]`
# subscript is stepped over as well, at every depth, and is covered separately
# (it wants the subscript's own string scope, which osh does not have yet).
#
# Verified against bash 5.2.37.

y=Y

echo "=== the scan reports whether or not the operand is used"
a='A${y:-p$(fi)q}B';  printf '1 [%s] rc=%s\n' "${a@P}" "$?"
b='A${y:+m$(fi)n}B';  printf '2 [%s] rc=%s\n' "${b@P}" "$?"
c='A${y:=p$(fi)q}B';  printf '3 [%s] rc=%s\n' "${c@P}" "$?"
d='A${y:?p$(fi)q}B';  printf '4 [%s] rc=%s\n' "${d@P}" "$?"
e='A${nope:-p$(fi)q}B'; printf '5 [%s] rc=%s\n' "${e@P}" "$?"

echo "=== the text named is the whole text, not the brace"
f='${y:-$(fi)}';      printf '6 [%s]\n' "${f@P}"
g='A${y:-p$(fi)q}';   printf '7 [%s]\n' "${g@P}"
h='A${y:-${z:-p$(fi)q}}B'; printf '8 [%s]\n' "${h@P}"

echo "=== everything before the brace has run; nothing in or after it has"
i='$(echo pre >&2)A${y:-p$(fi)q}B';  printf '9 [%s]\n' "${i@P}"
j='A${y:-$(echo in >&2)p$(fi)q}B';   printf '10 [%s]\n' "${j@P}"
k='A${y:-p$(fi)q}B$(echo post >&2)C'; printf '11 [%s]\n' "${k@P}"
l='A${y:-p$(fi)q}B${y}C';            printf '12 [%s]\n' "${l@P}"

echo "=== the scan steps over what it does not parse"
m='A${y:-p`fi`q}B';    printf '13 [%s]\n' "${m@P}"
n='A${y:-p$((1+))q}B'; printf '14 [%s]\n' "${n@P}"
# A `' … '` is skipped by the *scan* (`skip_single_quoted`, subst.c:1926-1938)
# but is not a quote to the *expansion*, which is past the point where one could
# be: the substitution runs and the quotes stay as text.
o='A${y:-'"'"'p$(fi)q'"'"'}B';       printf '15 [%s]\n' "${o@P}"
p='A${nope:-'"'"'p$(echo Q)q'"'"'}B'; printf '16 [%s]\n' "${p@P}"
# A `" … "` is handed to `skip_double_quoted`, which does read a `$(`.
q='A${y:-"p$(fi)q"}B'; printf '17 [%s]\n' "${q@P}"

echo "=== off the prompt path the read's jump stands and the brace never fails"
( cat <<E
A${y:-p$(fi)q}B
E
) 2>&1
echo "after=$?"
( cat <<E
A${y:+m$(fi)n}B
E
) 2>&1
echo "after=$?"

echo "=== a re-print that will not read back is the same read"
echo "A${y:-p$(!
)q}B"
echo "after=$?"

echo done
