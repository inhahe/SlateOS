# Inside `" … "` the brace scanner's `` ` `` row (braces.c:673) is unreachable —
# `quoted` is already `"` — so the opening backquote is only a character to it
# and it reads straight on into the body, where the `$(` row of the
# `quoted == '"'` branch still fires. A body that will not parse is therefore a
# `command substitution:` diagnostic from the *scan*, before the word is brace
# expanded and long before the backquote is ever run.
#
# But the scan's state is one flat byte, so a `"` inside the body is `c ==
# quoted` and clears it outright. After that a `'` or a `` ` `` opens a stretch
# the `$(` row cannot fire in, and a second `"` puts the state back. The body is
# therefore read in *stretches*, not whole — which is what the first case here
# measures: the substitution sits in a stretch the scan skips, so the word brace
# expands and the failure comes later, from running the backquote.
#
# The diagnostic's remainder tells the two apart. The scan was handed the whole
# word, so `extract_command_subst` there quotes the rest of the *word* — past the
# closing backquote and into the brace. The backquote's own run quotes only its
# own body.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the scan reads into the body, so the remainder runs past the word ==="
e 'echo "p`echo $(fi)`q{,}"'
e 'echo "p`echo "$(fi)"`q{,}"'
e 'echo "p`echo "x" $(fi)`q{,}"'
e 'echo "p`echo "x" $(fi) y`q{,}"'

echo "=== a quote inside the body clears the state and hides what follows ==="
e 'echo "p`echo '"'"'"'"'"' $(fi)`q{,}"'

echo "=== the < and > rows are unquoted-only, so the state decides them too ==="
e 'echo "p`echo "x" <(fi)`q{,}"'
e 'echo "p`echo "x" >(fi)`q{,}"'
e 'echo "p<(fi)q{,}"'

echo "=== at the top level the backquote row *is* reached, and skips whole ==="
e 'echo p`echo $(fi)`q{,}'
echo TAIL
