# What `[[ … =~ … ]]` accepts as a regex.
#
# The RHS is handed to the C library's ERE compiler, which is a good deal
# stricter than a "be liberal in what you accept" engine would be. A pattern it
# refuses is not a non-match — it is status 2, the same status a malformed
# conditional gets.
#
# The rules that are easy to get wrong, all of them measured:
#   * an unescaped `{` must open a well-formed interval — there is no falling
#     back to a literal brace;
#   * a quantifier needs a real atom in front of it, and `^` is not one (`$` is);
#   * one quantifier per atom;
#   * every branch of an alternation must be non-empty, even though a wholly
#     empty parenthesised subexpression is fine;
#   * `{0}` *deletes* the atom it follows rather than repeating it zero times,
#     and it is an error only if that leaves the concatenation with nothing;
#   * an empty pattern is an error.
#
# Statuses only: 0 matched, 1 did not, 2 was rejected.

r() { printf '%-22s ' "$1"; eval "$1" 2>/dev/null; printf 'st=%s\n' "$?"; }

echo "=== a brace either opens an interval or it is an error"
r '[[ ab =~ a{1}b ]]'
r '[[ ab =~ a{1,}b ]]'
r '[[ ab =~ a{1,2}b ]]'
r '[[ "a{b" =~ a{b ]]'
r '[[ "a{b" =~ a{1 ]]'
r '[[ "a{}b" =~ a{}b ]]'
r '[[ ab =~ a{,3} ]]'
r '[[ ab =~ a{1,2,3} ]]'
r '[[ "{b" =~ {b ]]'
r '[[ ab =~ a{3,1} ]]'
# ... and this is how you ask for a literal one.
r '[[ "a{b" =~ a\{b ]]'
r '[[ "a{b" =~ a[{]b ]]'

echo "=== a quantifier needs something to quantify"
r '[[ ab =~ *a ]]'
r '[[ ab =~ +a ]]'
r '[[ ab =~ ?a ]]'
r '[[ ab =~ {2}a ]]'
r '[[ ab =~ (*a) ]]'
r '[[ ab =~ a|*b ]]'
# `^` is an assertion, so there is nothing there to repeat; `$` is an atom.
r '[[ ab =~ ^*a ]]'
r '[[ ab =~ a^*b ]]'
r '[[ "a" =~ a$* ]]'

echo "=== ... and only one of them per atom"
r '[[ ab =~ a**b ]]'
r '[[ ab =~ a*+b ]]'
r '[[ ab =~ a*?b ]]'
r '[[ ab =~ a?+b ]]'
r '[[ ab =~ a{1}*b ]]'
r '[[ ab =~ a*{1}b ]]'
r '[[ ab =~ a{1}{2}b ]]'
r '[[ ab =~ (a)*+ ]]'

echo "=== every branch of an alternation is a real branch"
r '[[ ab =~ a|b ]]'
r '[[ ab =~ |a ]]'
r '[[ ab =~ a| ]]'
r '[[ ab =~ (a|) ]]'
r '[[ ab =~ (|a) ]]'
r '[[ ab =~ (a||b) ]]'
# An empty subexpression on its own is not a branch, and is allowed.
r '[[ ab =~ () ]]'
r '[[ ab =~ a()b ]]'

echo "=== a zero count deletes the atom"
r '[[ b =~ ^a{0}b$ ]]'
r '[[ b =~ ^a{0,0}b$ ]]'
r '[[ ab =~ a{0} ]]'
r '[[ ab =~ (a{0}) ]]'
r '[[ ab =~ a{0}|b ]]'
r '[[ ab =~ a{0}b{0} ]]'
# What is left over keeps the run alive.
r '[[ ab =~ ^a{0} ]]'
r '[[ ab =~ a{0}$ ]]'

echo "=== and an empty pattern is not a pattern"
r '[[ ab =~ "" ]]'
e=""; r '[[ ab =~ $e ]]'
