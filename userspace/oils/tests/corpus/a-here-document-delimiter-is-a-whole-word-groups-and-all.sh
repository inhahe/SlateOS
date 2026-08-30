# The word after `<<` is read by `read_token_word`, the same function that reads
# every other word, so a `$( … )`, `$(( … ))`, `${ … }` or `` ` … ` `` in it is
# scanned as a matched pair and becomes part of the delimiter. Two things follow
# that are easy to get backwards:
#
#   * Nothing is expanded. The delimiter's text is compared against each body
#     line literally, so `<<E$(echo 1)` is closed by a line reading
#     `E$(echo 1)` — not by `E1`. bash never runs the substitution; it only
#     has to *scan* it to know where the word ends.
#   * A separator inside a group is data. `<<E$(a b)` is one delimiter, not the
#     delimiter `E$(a` followed by a stray word `b)`, because the space is
#     inside the matched pair the scan is in the middle of.
#
# And a group is not a quote. Quoting the delimiter — `<<'E'`, `<<"E"`, `<<\E`
# — turns off expansion in the *body*; a group does not, not even when there
# are quotes inside it. `<<E$(echo "a b")` still expands `$x` in the body.
#
# The first half is about where the word ends and what it says, so each case
# closes its here-document properly and prints the body. The second half is
# about a group that never closes, which is fatal — and about which line bash
# blames for it, which is not the same for every kind of group.

x=1
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== a command substitution is part of the word, unexpanded"
cat <<E$(echo 1)
$x body
E$(echo 1)
echo "  rc=$?"

echo "=== …separators inside it and all"
cat <<E$(a b)
$x body
E$(a b)
echo "  rc=$?"

echo "=== arithmetic closes on the second paren, not the first"
cat <<E$((1+1))
$x body
E$((1+1))
echo "  rc=$?"

echo "=== a backquote pair is one too, and never nests"
cat <<E`a b`
$x body
E`a b`
echo "  rc=$?"

echo "=== so is a brace expansion, with its own separators"
cat <<E${y:-a b}
$x body
E${y:-a b}
echo "  rc=$?"

echo "=== a group can begin the word, and can be the whole of it"
cat <<$(echo x)
$x body
$(echo x)
echo "  rc=$?"

echo "=== quotes inside a group do not quote the word"
# `$x` is still expanded below: the `"a b"` is inside the substitution, and the
# delimiter itself was never quoted.
cat <<E$(echo "a b")
$x body
E$(echo "a b")
echo "  rc=$?"

echo "=== …whereas quoting the delimiter does, as ever"
cat <<"E$(echo 1)"
$x body
E$(echo 1)
echo "  rc=$?"

echo "=== and the group has to close"
# `parse_matched_pair`'s end-of-input path is fatal, so the delimiter word is
# *not* whatever was read up to there — the whole parse dies. Which line is
# blamed says which of bash's readers took the group, and they differ:
#
#   * `${` and `` ` `` are `parse_matched_pair` itself, which reports at the
#     `start_lineno` it captured on entry — the opener's own line.
#   * `$((` is `parse_matched_pair` too (`P_ARITH`), so likewise.
#   * `$(` alone is handed to `parse_comsub`, which parses the body as a command
#     and reports where *that* died: the end of the input.
#
# Each probe runs under `eval` in a subshell, both because a syntax error would
# otherwise abandon the rest of the input and because it puts the line numbers
# on a scale of their own, starting at the first line of the fragment.
e 'cat <<E$(
body
E$('
e 'cat <<E${
body
E${'
e 'cat <<E`
body'
e 'cat <<E$((
body'

echo "=== …and the innermost unclosed one is the one blamed"
# Line 3, not line 1: a nested `${` is a recursive `parse_matched_pair` call
# with a `start_lineno` of its own. The `$(` cases stay pinned to end-of-input
# however deep they nest, having no `start_lineno` in play at all.
e 'cat <<E${
body
E${'
e 'cat <<E$({
body
E$({'

echo "=== a quote inside a group has to close too, at its own line"
e 'cat <<E$(echo "a
b'
e "cat <<E\$(echo 'a
b"
