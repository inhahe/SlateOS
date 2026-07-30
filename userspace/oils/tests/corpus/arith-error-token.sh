# Every arithmetic diagnostic ends in `(error token is "…")`, and the token is
# not a token at all — it is the source text from wherever the parser was
# standing, running to the end of the expression. Which position that is depends
# on how the parse failed, and a parenthesised group has its own answer:
#
#   - a `)` that never arrives names the token the parser is standing on;
#   - at end of input it is standing on nothing, so it names the last token it
#     *did* read — which is the operand for `(2+3` but the `)` for `((2+3)`;
#   - a character the lexer has no token for is rejected by the lexer first, so
#     it is "invalid arithmetic operator" and never reaches the missing-`)`
#     check at all. Brackets count: `a[0]` is one token, so a stray `]` is not a
#     token, while `:` is.
#
# `let` is used throughout because it takes the expression as an argument, so
# an unbalanced `(` inside it is the arithmetic's problem and not the reader's.

echo "=== the token the parser is standing on ==="
let '( a b'
echo "rc=$?"
let '(1 2'
echo "rc=$?"
let '(2+3 4'
echo "rc=$?"
let '(1,2 3'
echo "rc=$?"
let '(1?2:3 4'
echo "rc=$?"
let '(x=1 2'
echo "rc=$?"
let '(a[0] b'
echo "rc=$?"
let '((1)(2)'
echo "rc=$?"
let '(2 3)'
echo "rc=$?"

echo "=== a nested group fails first, and reports from inside itself ==="
let '( (1 2) 3'
echo "rc=$?"
let '( ( a b ) )'
echo "rc=$?"
let '1 + ( a b )'
echo "rc=$?"
let '( a b ) + 1'
echo "rc=$?"

echo "=== at end of input, the last token read ==="
let '(2+3'
echo "rc=$?"
let '(1'
echo "rc=$?"
let '(a'
echo "rc=$?"
# The `-` is an operator, so the last token is the `1` after it.
let '(-1'
echo "rc=$?"
# A name with a subscript is one token.
let '(a[0]'
echo "rc=$?"
# Here the last token is the inner group's close paren, not its operand.
let '((2+3)'
echo "rc=$?"

echo "=== a character with no token is rejected before the missing paren ==="
let '(1 @'
echo "rc=$?"
let '(1;2'
echo "rc=$?"
let '(2+3]'
echo "rc=$?"

echo "=== and the same classification applies outside a group ==="
let '2+3]'
echo "rc=$?"
let '2+3['
echo "rc=$?"
let '2+3 @'
echo "rc=$?"
# `:` is a real token, so this one is an expression error, not an operator one.
let '2+3:'
echo "rc=$?"
let '2+3 4'
echo "rc=$?"
let '(2+3))'
echo "rc=$?"

echo "=== the shapes that already had an answer of their own ==="
# A missing operand reports from the operator that wanted it, not the cursor.
let '(1+'
echo "rc=$?"
let '2+3,'
echo "rc=$?"
# In operand position a bracket is an operand that is missing, not an operator.
let '1 + [2]'
echo "rc=$?"
# A balanced subscript is fine.
a=(7 8 9)
let 'a[0]'
echo "subscript rc=$?"

echo done
