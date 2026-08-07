# `<<` is a redirection operator and its target is an ordinary WORD — bash's
# grammar says so literally (`redirection: … LESS_LESS WORD`, parse.y). So a
# `<<` with no word after it is not a here-document with a strange delimiter;
# it is a *grammar error*, reported at whatever token turned up in the WORD's
# place, exactly as a `<` or a `>` with no target is.
#
# Which characters count as "a word here" is `read_token`'s business, and its
# very first act is the comment test: a `#` that *starts* a token opens a
# comment. `<<` is a metacharacter, so the character after it (or after the
# blanks following it) starts a token — and `cat <<#c` therefore has no
# delimiter at all. Once one character of the word has been read the `#` is
# just data, which is why `<<E#c` wants `E#c` and even `<<''#c` — where the
# quotes contributed *nothing* to the word but did start it — wants `#c`.
#
# Each probe runs in its own subshell, since a syntax error abandons the rest
# of the input it was read from.

echo "=== nothing at all after the operator"
( eval 'cat <<' ); echo "rc=$?"
( eval 'cat <<-' ); echo "rc=$?"
( eval 'cat << ' ); echo "rc=$?"

echo "=== the token that stands in the word's place is named"
( eval 'cat << ; echo hi' ); echo "rc=$?"
( eval 'cat << & ' ); echo "rc=$?"
( eval 'cat << | wc' ); echo "rc=$?"
( eval 'cat << >f' ); echo "rc=$?"
( eval 'cat << <<' ); echo "rc=$?"
( eval 'cat 3<< )' ); echo "rc=$?"
( eval 'cat {v}<< ;' ); echo "rc=$?"
( eval 'cat <<
echo after' ); echo "rc=$?"

echo "=== and inside every construct that reads one"
( eval 'f() { cat << ; }' ); echo "rc=$?"
( eval 'if true; then cat <<
fi' ); echo "rc=$?"
( eval 'while cat << ; do :; done' ); echo "rc=$?"
( eval 'case x in y) cat << ;; esac' ); echo "rc=$?"
( eval '{ cat << }' ); echo "rc=$?"
( eval '( cat << )' ); echo "rc=$?"

echo "=== a # after the operator is a comment, so there is no word"
( eval 'cat <<#c' ); echo "rc=$?"
( eval 'cat << #c' ); echo "rc=$?"
( eval 'cat <<-	#c' ); echo "rc=$?"

echo "=== but a # the word has already started is data"
( eval 'cat <<E#c
b
E#c
echo ok' ); echo "rc=$?"
( eval "cat <<''#c
b

echo ok" ); echo "rc=$?"
( eval 'cat << \#c
b
#c
echo ok' ); echo "rc=$?"

echo "=== an empty delimiter is fine when quoting made it one"
( eval 'cat <<""
line

echo ok' ); echo "rc=$?"
( eval "cat <<''
line

echo ok" ); echo "rc=$?"

# Not probed: `cat <<\` at the very end of the input, whose delimiter bash
# spells `\` — see TD-OILS-A-HERE-DOCUMENT-DELIMITER-OF-A-LONE-TRAILING-BACKSLASH.

echo "=== the operator itself is still fine with a word"
cat <<E
a body
E
cat <<-E
	stripped
	E
echo "done"
