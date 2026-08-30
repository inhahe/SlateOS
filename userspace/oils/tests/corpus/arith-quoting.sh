# Quotes in an arithmetic expression are removed by whoever *expands* the text,
# never by the arithmetic evaluator itself. The split is the same one that
# decides who expands (see arith-no-expansion.sh):
#
#   * a `(( … ))` command and a `for (( … ))` header are raw parser text, so the
#     shell expands them — and that pass removes double quotes and applies
#     double-quoting's backslash rules.
#   * a `let` argument, an integer-assignment value and a `[[ … -eq … ]]`
#     operand are ordinary words that were already word-expanded, so nothing
#     dequotes them a second time. Quotes that survive into the evaluator are
#     ordinary (invalid) characters and are rejected.
#
# Single quotes are never removed by either pass.

echo "=== the expansion pass removes double quotes ==="
echo "[$(( "3" + "4" ))]"
echo "[$(( 1"2"3 ))]"
q=7
(( "q" )); echo "var rc=$?"
(( "q" > 3 )); echo "cmp rc=$?"
for (( i="0"; i<"2"; i++ )); do echo "for i=$i"; done

echo "=== a value the evaluator reads for itself keeps them ==="
# A rejected `$(( … ))` abandons the rest of its list, so each status has to be
# read on a *new* list to be seen at all.
x='"3"'
echo "[$((x+1))]"; echo "unreachable-indirect"
echo "indirect rc=$?"
let 'y="3"+4'; echo "let rc=$?"
declare -i k='"3"+4'; echo "unreachable-declare"
echo "declare rc=$? k=[$k]"
[[ '"3"' -eq 3 ]]; echo "dbracket rc=$?"

echo "=== single quotes are nobody's to remove ==="
(( '3' )); echo "sq rc=$?"
s="'3'"
echo "[$((s))]"; echo "unreachable-sq"
echo "sq-indirect rc=$?"

echo "=== backslashes follow double-quoting's rules ==="
n=5
# The escape bites only before the four characters double quoting makes
# special, and it survives the expansion pass so the evaluator sees the plain
# character rather than an expansion.
(( \$n )); echo "dollar rc=$?"
(( "\$n" )); echo "quoted-dollar rc=$?"
(( \"q\" )); echo "quote rc=$?"
(( \`echo 1\` )); echo "backtick rc=$?"
(( 5 \\ 5 )); echo "backslash rc=$?"
# Anywhere else the backslash is an ordinary character and stays.
(( \q )); echo "other rc=$?"
# `\<newline>` is a line continuation, so both characters disappear.
(( 1 + \
2 )); echo "continuation rc=$?"

echo "=== a subscript is expanded the same way ==="
a=(zero one two)
echo "[${a["1"]}]"
echo "[${a[\$n]}]"
echo "sub-dollar rc=$?"
echo "[${a[\1]}]"
echo "sub-other rc=$?"
echo "[${a['1']}]"
echo "sub-sq rc=$?"
echo "[$(( a["1"] + 1 ))]"

echo done
