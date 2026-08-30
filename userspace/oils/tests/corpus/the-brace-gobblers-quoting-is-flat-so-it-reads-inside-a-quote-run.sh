# The brace scanner (`brace_gobbler`, braces.c:637-682) is the *earliest* reader
# of a command substitution, and its quoting state is one flat byte:
#
#     if (quoted) { if (c == quoted) quoted = 0;
#                   if (quoted == '"' && c == '$' && text[i+1] == '(') goto comsub; … }
#     if (c == '"' || c == '\'' || c == '`') { quoted = c; … }
#
# Inside `" … "` that byte is `"`, so the `'` row is never reached: a `' … '` run
# is not a quote to this reader at all, and the `$(` row fires on what is between
# the quotes. bash's *parser* did read the run as an extent it never looked into
# (parse.y:3840), so the two readers disagree by design — and the gobbler's read
# happens first. A body that will not parse is therefore a `command substitution:`
# diagnostic and a discarded command, even though the parse skipped it whole.
#
# Nothing about the gobbler's state nests, which is the second half of this:
#
#   * a `"` met while the state is `"` *clears* it rather than opening a level,
#     and the `$(` row still fires afterwards, because `0` reaches it too;
#   * but with the state cleared, a backquote opens a stretch the `$(` row cannot
#     fire in — so the very same body, one character further along, is never read
#     and the command runs.
#
# Each case is run in a subshell so a discarded command takes only that subshell
# with it. Verified against bash 5.2.37.

unset z
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== inside double quotes the run is read, so its body must parse ==="
e 'echo "A[${z#'\''$(echo Q)'\''}]"'
e 'echo "B[${z#'\''$(fi)'\''}]"'
e 'echo "C[${z:-'\''$(fi)'\''}]"'
e 'echo "D[${z//x/'\''$(fi)'\''}]"'
e 'echo "E[${z%'\''$(fi)'\''}]"'

echo "=== a dquote inside the run clears the state, it does not nest ==="
e 'echo "F[${z#'\''a"$(fi)'\''}]"'
e 'echo "G[${z#'\''a"b"$(fi)'\''}]"'

echo "=== …but once cleared, a backquote hides the body from the scan ==="
e 'echo "H[${z#'\''a"`$(fi)`'\''}]"'
e 'echo "I[${z#'\''a"`x`$(fi)'\''}]"'

echo "=== a backslash passes the next byte over, hiding a quote ==="
e 'echo "J[${z#'\''a\`$(fi)'\''}]"'

echo "=== outside double quotes the run is a quote and nothing is read ==="
e 'echo K[${z#'\''$(fi)'\''}]'
e 'echo L[${z:-'\''$(fi)'\''}]'
echo TAIL
