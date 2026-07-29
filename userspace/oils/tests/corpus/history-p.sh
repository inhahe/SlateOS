# `history -p` — the one way to reach `!`-style history expansion without
# turning it on. The expansion engine itself is pinned by histexpand.sh and the
# `set -H` switch by histexpand-option.sh; this case is about the *builtin*,
# which differs from the reader in three observable ways:
#
#   * it expands whatever `set -H` says, so everything below runs with the
#     switch off;
#   * it un-records its own line first, so `!!` names the command before it;
#   * a failure is reported as `history: ARG: history expansion failed` naming
#     the whole argument — not the reader's `EVENT: event not found` naming the
#     event — and it does not stop the remaining arguments.

echo "=== with no history at all the expansion simply fails"
history -p '!!'; echo "  rc=$?"

set -o history

echo "=== !! is the command before the -p line, not the -p line itself"
echo alpha beta
history -p '!!'; echo "  rc=$?"

echo "=== word designators and quick substitution work the same as in a line"
echo one two three
history -p '!$' '!^' '!!:2' 'x!$y'
history -p '^one^ONE^'

echo "=== single quotes still suppress, and a plain word passes through"
history -p "'!!'" plain ''

echo "=== a :p modifier prints like any other: -p never runs anything"
echo gamma
history -p '!!:p'; echo "  rc=$?"

echo "=== an unknown event names the whole argument and returns 1"
history -p 'aa !nosuchevent bb'; echo "  rc=$?"

echo "=== one bad argument does not stop the good ones on either side"
echo delta
history -p 'before !!' '!nosuchevent' 'after !!'; echo "  rc=$?"

echo "=== a malformed modifier reports the same way"
history -p '!!:s/x'; echo "  rc=$?"

echo "=== the last substitution is shared, so :& repeats it"
echo aa bb
history -p '!!:s/aa/cc/'
history -p '!!:&'

echo "=== none of that was recorded: only the echoes are in the list"
history
