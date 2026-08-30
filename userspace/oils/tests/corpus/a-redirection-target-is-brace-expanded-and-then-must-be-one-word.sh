# A redirection target is a *word*, and the word list it belongs to is expanded
# by `expand_words_no_vars` — `expand_word_list_internal (list, WEXP_NOVARS)`
# (subst.c:12787). `WEXP_NOVARS` is
#
#     #define WEXP_NOVARS (WEXP_ALL & ~WEXP_VARASSIGN)   /* subst.c:12690 */
#
# — everything but the variable-assignment pass, so `WEXP_BRACEEXP` survives and
# `redirection_expand` (redir.c:214-232) gets back however many words the braces
# made. It then insists on exactly one:
#
#     if (result == 0 || result->next != 0) { dispose_words (result); return 0; }
#
# and a null return is `AMBIGUOUS_REDIRECT`, reported as `%s: ambiguous
# redirect` naming the word *as written* — braces and all, because the name in
# the message is the unexpanded `redirectee.filename->word`.
#
# So the count is what matters, not the braces: `> h{a,a}` expands to two words
# that happen to be equal and is still ambiguous, while `> g{1}` was never a
# brace expansion at all (one comma-free element) and simply opens that file.
#
# Being an ordinary brace-expanded word also brings the target under
# `brace_gobbler`'s command-substitution read — the last section here — and that
# read failing is a *fatal* expansion error, which abandons the word rather than
# handing back zero fields, so it is not reported as an ambiguity.
#
# Verified against bash 5.2.37.

rm -f f1 f2 'f{1,2}' g1 'g{1}' ha 'h{a,a}' p1 'p{1,2}' q1 'q{1,2}'

echo "=== a target that expands to more than one word is ambiguous ==="
echo hi > f{1,2}
echo "1 rc=$?"
echo "1 files=[$(echo f*)]"

echo "=== …and one that expands to exactly one is not ==="
echo hi > g{1}
echo "2 rc=$?"
cat 'g{1}'
echo hi > h{a,a}
echo "3 rc=$?"
echo "3 files=[$(echo h*)]"

echo "=== every write form counts the same way ==="
echo hi >> p{1,2}
echo "4 rc=$?"
echo hi >| q{1,2}
echo "5 rc=$?"
cat < f{1,2}
echo "6 rc=$?"
exec 3>&{1,2}
echo "7 rc=$?"

echo "=== a here-string is not a redirection target and is exempt ==="
cat <<< a{1,2}
echo "8 rc=$?"

echo "=== and what brace expansion never touches is untouched here too ==="
echo hi > "n{1,2}"
echo "9 rc=$? files=[$(echo n*)]"
set +B
echo hi > 'k'{1,2}
echo "10 rc=$? files=[$(echo k*)]"
set -B

echo "=== the brace scanner reads the target's command substitutions too ==="
unset z
echo hi > "/dev/null${z:-'$(fi)'}"
echo "11 rc=$?"
echo TAIL
