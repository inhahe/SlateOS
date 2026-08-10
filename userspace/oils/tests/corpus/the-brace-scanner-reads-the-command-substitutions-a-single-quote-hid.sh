# The scanner that looks for the `{` a brace expansion starts at is also a
# reader of command substitutions, and the earliest one there is. `brace_gobbler`
# (braces.c:637-682) treats a `${` like `\{` — it opens no quoting state of its
# own — so between the braces the *word's* quoting is still what is in force.
# Inside `" … "` that means a `'` is not a quote, and every `$( … )` it was
# hiding gets parsed here, by `extract_command_subst` → `xparse_dolparen`.
#
# That is the one construct bash's parser never read (a `' … '` inside a
# double-quoted `${ … }` is an extent it skips, not source), so a body that will
# not parse is not a script syntax error but a runtime diagnostic: `command
# substitution: line N:` followed by the remainder of the *word*, and then
# `jump_to_top_level (DISCARD)` — the one command is dropped with `$?` at 1 and
# the shell carries on.
#
# Three properties of that ending belong to this scan and to no other reader:
#
#   * it is gated on `set -B`, because it *is* brace expansion's scanner;
#   * it reaches only the words brace expansion reaches — a command word, a
#     `for` list, a `declare` argument and a redirection target — the last
#     because `expand_words_no_vars` is `WEXP_NOVARS`, which is `WEXP_ALL &
#     ~WEXP_VARASSIGN` and so still carries `WEXP_BRACEEXP` — but not a plain
#     assignment, a `case` word or a here-document body, which report from the
#     operand's own read instead and quote the operand's shorter remainder.
#   * it stops at the first failure, so a word with two failing substitutions
#     reports once.
#
# The remainder it quotes is the whole word with nothing cut out of it — a
# `[ … ]` subscript included, which the `${ … }` scan steps over but this one
# reads straight through. Those rows are missing here because osh does not yet
# parse what a `' … '` in a subscript or a substring bound holds; see
# TD-OILS-A-SINGLE-QUOTED-RUN-IN-A-BARE-SUB-WORD-OF-A-BRACE-IS-A-QUOTE.
#
# Verified against bash 5.2.37.

unset z

echo "=== the scan reads what the parser skipped ==="
echo "p${z:-'$(fi)'}q"
echo "1 rc=$?"

echo "=== …and quotes the rest of the word, however deep the read was ==="
echo "p${z:-${z:-'$(fi)'}}q"
echo "2 rc=$?"
echo "p${z:-'$(fi)'}q${z}r"
echo "3 rc=$?"

echo "=== it stops at the first failure ==="
echo "p${z:-'$(fi)'}q${z:-'$(for)'}r"
echo "4 rc=$?"

echo "=== a single quote at the *top* level still hides it ==="
echo p${z:-'$(fi)'}q
echo "5 rc=$?"

echo "=== set +B turns the scan off, and the operand's own read reports ==="
set +B
echo "p${z:-'$(fi)'}q"
echo "6 rc=$?"
set -B

echo "=== only the words brace expansion reaches are scanned ==="
v="p${z:-'$(fi)'}q"
echo "7 rc=$? v=[$v]"
declare w="p${z:-'$(fi)'}q"
echo "8 rc=$? w=[$w]"
for i in "p${z:-'$(fi)'}q"; do echo "9 body $i"; done
echo "9 rc=$?"
case "p${z:-'$(fi)'}q" in *) echo "10 body";; esac
echo "10 rc=$?"
echo hi > "/dev/null${z:-'$(fi)'}"
echo "11 rc=$?"
cat <<EOF
p${z:-'$(fi)'}q
EOF
echo "12 rc=$?"

echo "=== a body that parses is read and thrown away, not run ==="
echo "p${z:+'$(echo SIDE)'}q"
echo "13 rc=$?"
echo "p${z:-'$(echo SIDE)'}q"
echo "14 rc=$?"
echo TAIL
