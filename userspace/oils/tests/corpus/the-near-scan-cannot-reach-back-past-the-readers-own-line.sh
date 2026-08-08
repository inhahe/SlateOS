# `error_token_from_text` (parse.y) scans one line, so the scan has a floor.
#
# It opens with `t = shell_input_line`, and that buffer holds a *single* line:
# `shell_getc` replaces it — it does not extend it — after deleting a
# `\<newline>` and going back round (`goto restart_read`). So a reader dragged
# onto a fetched line is at index 0 of a fresh `t`, where every one of bash's
# back-scan loops is guarded by `i > 0` and none of them runs. `token_end`
# stays 0 and the final branch returns the single character `t[0]`.
#
# With a non-blank fetched line that is the same answer a walk over the whole
# input would give — `echo 2` yields `e` from either — so only a blank line
# tells them apart. There the character is the fetched line's own newline (or
# its first blank), never the token back on the line before.
#
# The discriminator is therefore what stands on the continuation line, not what
# is on the line that failed: `[[ a == b )\` reports near the newline with an
# empty line under it, near a space with `   ` under it, near `;` with `;`
# under it — all for the same `)`.
#
# A read that stopped mid-line never fetched at all, so its buffer is still the
# line it started on: `[[ a -eq b -eq c\` keeps reporting near `-eq` and
# echoing its own line, blank continuation or not.
#
# Verified against bash 5.2.37.

mk() { f=$1; shift; printf '%s\n' "$@" > "$f"; }

echo "=== the fetched line is blank, so the newline itself is the near text"
mk a1 'echo 1' '[[ a == b )\' ''
. ./a1
mk a2 'echo 1' '[[ a b\' ''
. ./a2
mk a3 'echo 1' '[[ -z x y\' ''
. ./a3
mk a4 'echo 1' '[[ a == b ;\' ''
. ./a4
mk a5 'echo 1' '[[ a ==\' ''
. ./a5
mk a6 'echo 1' '[[ ( a ;\' ''
. ./a6

echo "=== and a blank line still ends the parse, whatever follows it"
mk b1 'echo 1' '[[ a == b )\' '' 'echo 2'
. ./b1

echo "=== whitespace is text, so index 0 of it is what gets named"
mk c1 'echo 1' '[[ a == b )\' '   '
. ./c1
mk c2 'echo 1' '[[ a == b )\' '	'
. ./c2
mk c3 'echo 1' '[[ a == b )\' ' x'
. ./c3
mk c4 'echo 1' '[[ a == b )\' ';'
. ./c4

echo "=== a non-blank line agrees with a whole-input walk by accident"
mk d1 'echo 1' '[[ a == b )\' 'echo 2'
. ./d1
mk d2 'echo 1' '[[ a == b ;\' 'echo 2'
. ./d2

echo "=== a read that stopped mid-line never fetched, so its own line stands"
mk e1 'echo 1' '[[ a -eq b -eq c\' ''
. ./e1
mk e2 'echo 1' '[[ a == b ) \' ''
. ./e2
mk e3 'echo 1' '[[ a == b )'
. ./e3

echo "=== the general branch is not a near scan at all, and echoes an empty line"
mk f1 'echo 1' ';;\' ''
. ./f1
mk f2 'echo 1' 'echo a; )\' ''
. ./f2
