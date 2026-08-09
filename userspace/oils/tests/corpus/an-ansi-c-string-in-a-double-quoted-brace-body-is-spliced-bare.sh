# A `$'…'` in a `${ … }` body is translated where it is read, always. What is
# *not* always the same is whether the translation is re-quoted afterwards, and
# bash makes that a three-way decision on the scan's `P_DQUOTE` and its
# `dolbrace_state`:
#
#   | where the `$'…'` sits                            | re-quoted? | parse.y |
#   |--------------------------------------------------|------------|---------|
#   | no `P_DQUOTE` — the `${` was not written in `"…"` | yes        | 3882    |
#   | `P_DQUOTE`, past a `#`/`%`/`/`/`^`/`,`            | yes        | 3866    |
#   | `P_DQUOTE`, still in the name or in a `:-` word   | **no**     | 3887    |
#
# The third row splices the translation in bare, and bash has that fix written
# and disabled behind `#if 0 /* TAG:bash-5.3 */` (parse.y:3875) — a deliberate
# decision to keep 5.2 answering this way, so it is behaviour and not an
# unchecked defect.
#
# What the bare splice costs is the value's quoting, and the cost is paid by
# whoever reads the body next. bash reads a word twice: `parse_matched_pair`
# only looks for the end of the *word*, and never re-reads what it accumulated,
# so a `}` the translation contributed terminates nothing there. The word text
# is then handed to `parameter_brace_expand`, which scans it fresh — and *that*
# scan meets the spliced `}` like any other and closes the expansion on it,
# leaving the rest as ordinary word text:
#
#   x=; echo "${x:-$'a}b'}"          →  ab}      (`${x:-a}` and a literal `b}`)
#
# A spliced `'` is the same story with the opposite ending: it swallows the `}`
# the user wrote, the scan runs off the end of the word, and the expansion dies
# with ``bad substitution: no closing `}'`` naming the word *as translated*.
#
# `dolbrace_state` is what tells the second row from the third, and it is the
# machine parse.y and subst.c share ("This logic must agree", parse.y:3803):
# `DOLBRACE_PARAM` until an operator, `DOLBRACE_QUOTE` on a `#`/`%`/`/`/`^`/`,`
# that is not the body's first character, `DOLBRACE_OP` then `DOLBRACE_WORD`
# otherwise. bash appends the character *before* running the machine
# (parse.y:3785 → 3809), so its `retind > 1` test is satisfied by a name as
# short as one letter — `"${q#$'…'}"` re-quotes exactly as `"${qq#$'…'}"` does.
# A nested `${` recurses (parse.y:3928) and so starts a fresh `DOLBRACE_PARAM`
# while inheriting `P_DQUOTE`; a nested `$(` strips `P_DQUOTE` instead
# (parse.y:3959).
#
# Verified against bash 5.2.37.

p() { printf '%-42s -> [%s]\n' "$1" "$2"; }
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

unset x q qq a b

echo "=== bare: the third row splices the translation unquoted ==="
# The `}` closes the expansion at the second read, so `b}` is literal text.
p 'a closing brace'      "${x:-$'a}b'}"
p 'with text around it'  A"${x:-$'a}b'}"B
# Everything past the close is live word text, expansions and all.
y=Y
p 'an expansion after it' "${x:-$'a}b'$y}"
p 'a substitution after'  "${x:-$'a}b$(echo Z)'}"
# Not the operand only: the name is `DOLBRACE_PARAM`, which is the third row too.
p 'a tab, unquoted'       "${x:-$'a\tb'}"
p 'a glob character'      "${x:-$'a*b'}"

echo "=== quoted: the other two rows keep sh_single_quote ==="
# Row two — a `#` past the first character has moved to `DOLBRACE_QUOTE`.
q=zabz; qq=zabz
p 'one-letter name, #'   "${q#$'z'}"
p 'two-letter name, #'   "${qq#$'z'}"
p 'a % operator'         "${q%$'z'}"
v=abc
p 'a / operator'         "${v/b/$'p}q'}"
# Row one — the same body written outside the quotes.
p 'no double quotes'     ${x:-$'a}b'}

echo "=== a spliced quote leaves the brace unclosed ==="
e 'echo "${x:-$'\''\x27'\''}"'
e 'echo "${x:-$'\''a\x27b'\''}"'
# The double quote and the backtick are as live as the quote is, and each dies
# where the second read runs out of word.
e 'echo "${x:-$'\''a"b'\''}"'
e 'echo "${x:-$'\''a`b'\''}"'

echo "=== nesting: a fresh state inside, the same P_DQUOTE ==="
# The inner `${` recurses into `DOLBRACE_PARAM`, so its `:-` word is row three
# and its `}` closes the *outer* expansion at the second read.
p 'inner splices bare'   "${a:-${b:-$'x}y'}}"
# `${a#…}` is row two for the outer body, but the recursion resets it, so the
# inner is row three all the same.
p 'outer is # , inner :-' "${a#${b:-$'x}y'}}"
# The inner `${b#…}` is row two on its own account: re-quoted.
p 'inner is #'           "${a:-${b#$'x'}}"

echo "=== the printback shows the translation, spliced as it stands ==="
f() {
  echo "${x:-$'a}b'}"
  echo "${x:-$'\x27'}"
  echo "${x:-$'a"b'}"
  echo "${x#$'a}b'}"
  echo "${x:-$'a}b'$y}"
}
declare -f f

echo "=== the status is the expansion's, not the parser's ==="
e 'echo "${x:-$'\''a}b'\''}"; echo after'
e 'set -e; echo "${x:-$'\''\x27'\''}"; echo after'

echo done
