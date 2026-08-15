# `brace_gobbler` — the scan that looks for the `{` a brace expansion starts at
# — is also a reader of process substitutions, and its comsub row names all
# three spellings in one breath: `(c == '$' || c == '<' || c == '>') &&
# text[i+1] == '('` (braces.c:675), reached only while `quoted` is 0. The
# `quoted` branch has a row of its own and it is the `$(` alone (braces.c:668).
#
# Nothing about that state nests. A `"` met while `quoted` is `"` clears it
# outright, and a `${` opens no state at all (it is treated like `\{`), so the
# *inner* quotes of `"${z:-"<(fi)"}"` leave the scan unquoted over the `<(` and
# it parses what follows. The parser did not: its `" … "` is a recursive
# `parse_matched_pair` with no such row, so it took those bytes for characters.
# Hence a runtime diagnostic where one `"` further along — `"${z:-"a"<(fi)"b"}"`,
# whose `<(` the parity leaves inside the quotes — is a script syntax error, the
# parser having reached that one.
#
# Three properties of that ending are this scan's, as they are for the `$(`
# spelling: it is gated on `set -B`, it reaches only the words brace expansion
# reaches, and it happens before any of the word is expanded — so an operand
# that is never used is read all the same.
#
# The two questions this does not answer are the parse's and the expansion's,
# and each is its own case: see
# a-process-substitution-in-a-brace-body-is-parsed-where-it-is-met.sh and
# a-process-substitution-in-a-brace-body-is-performed-unless-the-expansion-is-quoted.sh.
# Nothing below prints a substitution's path — bash names it `/dev/fd/N` and osh
# a temporary file.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the inner quote clears the state, so the <( is this scan's ==="
e 'unset z; echo "${z:-"<(fi)"}"'
e 'unset z; echo "${z:-">(fi)"}"'
e 'unset z; echo "${z#"<(fi)"}"'
e 'unset z; echo "${z/x/"<(fi)"}"'
e 'unset z; echo "${z:0:"<(fi)"}"'
e 'a=(x); echo "${a["<(fi)"]}"'
e 'unset z; echo "${z:-"<(fi"}"'

echo "=== text this scan does not read keeps its characters ==="
e 'unset z; echo "${z:-"a"<(fi)"b"}"'
e 'unset z; unset y; echo "${z:-"${y:-"<(fi)"}"}"'
e 'unset z; echo "${z:-"'"'"'<(fi)'"'"'"}"'
e 'unset z; echo "${z:-"\<(fi)"}"'
e 'unset z; echo ${z:-"<(fi)"}'
e 'unset z; echo "${z:-"`echo x`<(fi)"}"'

echo "=== only where brace expansion runs, and only when it is on ==="
e 'unset z; x="${z:-"<(fi)"}"; echo assigned'
e 'unset z; case x in "${z:-"<(fi)"}") echo m ;; *) echo n ;; esac'
e 'unset z; set +B; echo "${z:-"<(fi)"}"'
e 'unset z; for w in "${z:-"<(fi)"}"; do echo "$w"; done'
e 'unset z; declare -a v=("${z:-"<(fi)"}"); echo ok'
e 'unset z; : "${z:-"<(fi)"}"'
e 'unset z; cat <<EOF
"${z:-"<(fi)"}"
EOF'

echo "=== read before the word is expanded, and read either way ==="
e 'z=Z; echo "${z:-"<(fi)"}"'
e 'unset z; echo "${z:+"<(fi)"}"'
e 'unset z; echo "${z:-"<(echo hi)"}"'
e 'unset z; echo "${z:-"<(echo hi)"}"{a,b}'
f() { echo "${z:-"<(echo hi)"}"; }; declare -f f

echo "=== in the order the scan meets them, beside the other spelling ==="
e 'unset z; echo "${z:-"<(fi)""<(for)"}"'
e 'unset z; echo "${z:-'"'"'$(fi)'"'"'"<(for)"}"'
e 'unset z; echo "${z:-"<(fi)"'"'"'$(for)'"'"'}"'
e 'unset z; echo "${z:-"<(fi)"}"; echo next'
echo TAIL
