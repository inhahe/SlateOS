# `extract_dollar_brace_string` names `$(`, `<(` and `>(` in **one row** and
# hands all three to `extract_command_subst` (subst.c:1881-1950). So a
# process substitution written between braces is *parsed for its extent* by the
# scan, and a body that will not parse is reported there — even though the
# expansion that follows performs only the `$(` spelling and prints the other
# two straight back as text.
#
# The scan is a second, separate pass over the undecoded word, and its quote
# rules are not the expansion's:
#
#   * a `' … '` run is stepped over **whole** (`skip_single_quoted`,
#     subst.c:1926-1938) — nothing inside it is read;
#   * a `" … "` run goes to `skip_double_quoted`, which has a row for `$(` and
#     `${` and **no row for `<(` or `>(`**;
#   * neither skipper knows the other's character, so a `"` inside a `' … '`
#     run is an ordinary byte and vice versa;
#   * a `\` hides the byte after it.
#
# That last set of rules is the whole content of this case, because the
# *expansion* disagrees with every one of them: to `expand_word_internal` a `'`
# is an ordinary character (quote removal is long past) and a `"` is a real
# quote. So the two passes carve the same operand differently, and it is the
# scan's carving that decides which reads happen and therefore what is
# reported.
#
# Verified against bash 5.2.37.

z=ZZ

echo "=== the plain row: read at brace level, never performed ==="
v='A${z:-P1<(fi
S1}B'; printf '[%s]\n' "${v@P}"
v='A${z:->(echo hi
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== a backslash hides the opener ==="
v='A${z:-P1\<(echo hi
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== …and a backslash spent on a quote leaves the opener live ==="
v='A${z:-P1\'"'"'<(echo hi
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== a squote inside a closed dquote run is inert, so the run closes ==="
v='A${z:-"it'"'"'s"$(fi
S1}B'; printf '[%s]\n' "${v@P}"
v='A${z:-"it'"'"'s"<(fi
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== a dquote inside a closed squote run is inert, likewise ==="
v='A${z:-'"'"'i"t'"'"'<(fi
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== a lone dquote: skip_double_quoted reads the dollar spelling only ==="
v='A${z:-"P1<(echo hi
S1}B'; printf '[%s]\n' "${v@P}"
v='A${z:-"P1$(echo hi
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== a nested brace is scanned by the same pass ==="
v='A${z:-${y:-<(fi
S1}B'; printf '[%s]\n' "${v@P}"

echo "=== a run the scan steps over whole reads nothing at all ==="
v='A${z:-'"'"'p<(fi)q'"'"'}B'; printf '[%s]\n' "${v@P}"
v='A${z:-'"'"'p$(fi)q'"'"'}B'; printf '[%s]\n' "${v@P}"

echo "=== a closed dquote run hides the procsub but not the comsub ==="
v='A${z:-"p<(fi)q"}B'; printf '[%s]\n' "${v@P}"

echo "=== the same operand in a here-document body ==="
cat <<E
A${z:-P1<(fi
S1}B
E

echo TAIL
