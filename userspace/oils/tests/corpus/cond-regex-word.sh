# Where the RHS of `[[ … =~ … ]]` ends.
#
# The regex is read as one word, so most of the regex alphabet — `|`, `*`, `^`,
# `$`, `#`, `}` — is literal rather than shell syntax. The exception is a `( … )`
# group: while one is open the word swallows everything, blanks and shell
# operators alike, which is what lets a regex contain a space at all. Once the
# group closes the ordinary word boundaries are back, so `;`, `&`, `<`, `>`, a
# blank and a stray `)` all end the RHS and are read as shell tokens — which
# inside `[[ ]]` means a conditional-expression syntax error.
#
# Quoting is orthogonal: a quoted or backslash-escaped paren is a literal one and
# neither opens nor closes a group, and a fully quoted RHS is matched as a
# literal string.
#
# Statuses only, mostly: 0 matched, 1 did not, 2 was rejected.

r() { printf '%-26s ' "$1"; eval "$1" 2>/dev/null; printf 'st=%s\n' "$?"; }

echo "=== a group holds on to the blanks inside it"
r '[[ "a b" =~ (a b) ]]'
r '[[ "xa by" =~ x(a b)y ]]'
r '[[ "a b" =~ ((a b)) ]]'
r '[[ "a b" =~ ^(a b)$ ]]'
r '[[ "a b" =~ (a b)|(c) ]]'
r '[[ "a  b" =~ (a  b) ]]'
# The blanks are kept verbatim, not folded or trimmed.
[[ "x a b y" =~ ( a b ) ]] && echo "  match=[${BASH_REMATCH[0]}] group=[${BASH_REMATCH[1]}]"
r '[[ "a b" =~ ( a b ) ]]'

echo "=== ... and the shell operators too"
r '[[ "a;b" =~ (a;b) ]]'
r '[[ "a&b" =~ (a&b) ]]'
r '[[ "a>b" =~ (a>b) ]]'
r '[[ "a|b" =~ (a|b) ]]'
r '[[ "a]]b" =~ (a]]b) ]]'
# A newline inside the group is part of the regex, so the `[[` is still open.
r '[[ ab =~ (a
b) ]]'

echo "=== but outside one the word ends where a word ends"
r '[[ "a;b" =~ a;b ]]'
r '[[ "a&b" =~ a&b ]]'
r '[[ "a>b" =~ a>b ]]'
r '[[ "a<b" =~ a<b ]]'
r '[[ ab =~ a) ]]'
r '[[ ab =~ )a ]]'
r '[[ "a b c" =~ (a b) c ]]'
r '[[ ab =~ ab&& ]]'
# A group left open is the *word reader's* error, not the regex engine's.
r '[[ ab =~ (a(b) ]]'

echo "=== ... while these are just regex characters"
r '[[ ab =~ a|b ]]'
r '[[ ab =~ a*b ]]'
r '[[ "a#b" =~ a#b ]]'
r '[[ "a}b" =~ a}b ]]'
r '[[ ab =~ ^ab$ ]]'

echo "=== a quoted or escaped paren is a literal one"
r '[[ "a)b" =~ (a\)b) ]]'
r '[[ "ab" =~ (a"  "b) ]]'
r '[[ "a b" =~ "(a b)" ]]'
r '[[ "(a b)" =~ "(a b)" ]]'
# Expansion still happens, and a group can come from a variable.
v="q r"; r '[[ "q r" =~ ($v) ]]'
re='(a b)'; r '[[ "a b" =~ $re ]]'

echo "=== and grouping still works where it is not a regex"
r '[[ ( a == a ) ]]'
r '[[ ( a == a || b == c ) && d == d ]]'
r '[[ ab =~ (ab) && a == a ]]'
r '[[ ab =~ (a b) && a == a ]]'

echo "=== the captures are the group's own"
[[ "ab" =~ (a)(b) ]] && echo "  [${BASH_REMATCH[1]}][${BASH_REMATCH[2]}]"
# Two groups with a blank between them are two words, so this is rejected — the
# blank is only swallowed *inside* a group. (stderr dropped: bash's `syntax error
# near' echoes a raw source span, osh the token — see TD-OILS-COND-ERRTEXT.)
r '[[ "a b" =~ (a) (b) ]]'
