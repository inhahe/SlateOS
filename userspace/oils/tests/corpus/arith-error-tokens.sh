# bash's arithmetic diagnostic echoes the expression and names an "error
# token", and which token that is depends on the failure: most eval-time
# failures point back at the operand that caused them, but `**` points at
# whatever token the lexer happens to be holding — which, because the lexer
# reads one token ahead, is the one *after* the exponent.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== division by zero names its right operand, from the operand's start"
( echo $(( 1/0 )) ) 2>&1 | e
( echo $(( 1%0 )) ) 2>&1 | e
( echo $(( 1/(0) )) ) 2>&1 | e
( echo $(( 1/0/0 )) ) 2>&1 | e
( echo $(( 1/0+5 )) ) 2>&1 | e
( echo $(( 1%0*3 )) ) 2>&1 | e

echo "=== a negative exponent names the token that follows it"
( echo $(( 2**-1+9 )) ) 2>&1 | e
( echo $(( 2**-1*8 )) ) 2>&1 | e
( echo $(( 1+2**-3+4 )) ) 2>&1 | e
( echo $(( 2**-1==0 )) ) 2>&1 | e
( echo $(( 1?2**-1:0 )) ) 2>&1 | e
( echo $(( 2**-1,9 )) ) 2>&1 | e
( echo $(( 2**(-1)+7 )) ) 2>&1 | e

echo "=== …and at the end of the expression there is no next token"
( echo $(( 2**-1 )) ) 2>&1 | e
( echo $(( 5 ** -1 )) ) 2>&1 | e
( echo $(( 2** - 1 )) ) 2>&1 | e
( echo $(( x=2**-1 )) ) 2>&1 | e
( echo $(( 2**(-1) )) ) 2>&1 | e
( echo $(( 2**-(1) )) ) 2>&1 | e
( echo $(( (2**-1) )) ) 2>&1 | e
# Right-associative, so the inner exponentiation fails first.
( echo $(( 2**2**-1 )) ) 2>&1 | e
# A too-large exponent is a different complaint.
( echo $(( 2**4294967296 )) ) 2>&1 | e

echo "=== a variable's value is re-lexed exactly as stored"
# Only the leading whitespace is skipped — once by the lexer, once again when
# the value is echoed. The trailing whitespace stays in both the echoed
# expression and the error token.
( x='1 + '; echo $(( x )) ) 2>&1 | e
( x='  1 +  '; echo $(( x )) ) 2>&1 | e
( x='1 +'; echo $(( x )) ) 2>&1 | e
( x=' 1/0 '; echo $(( x )) ) 2>&1 | e
( x=' 2**-1 '; echo $(( x )) ) 2>&1 | e
( x=' 5 apples '; echo $(( x )) ) 2>&1 | e
( x=' 1 @ '; echo $(( x )) ) 2>&1 | e
( x=' x '; echo $(( x )) ) 2>&1 | e
( a=' b '; b=' a '; echo $(( a )) ) 2>&1 | e

echo "=== whitespace around a value that does evaluate is invisible"
( x=' 5 '; echo $(( x )) ) 2>&1 | e
( x=' 010 '; echo $(( x )) ) 2>&1 | e
( x=' 0x10 '; echo $(( x )) ) 2>&1 | e
( x='  '; echo $(( x )) ) 2>&1 | e
( x=' -7 '; echo $(( x )) ) 2>&1 | e
( x=' 2+3 '; echo $(( x + 1 )) ) 2>&1 | e

echo "=== the same tokens through let and (( ))"
( let 'x = 2**-1'; echo "rc=$?" ) 2>&1 | e
( (( 1/0 )); echo "rc=$?" ) 2>&1 | e
( v=' 1 + '; let "v"; echo "rc=$?" ) 2>&1 | e

echo "=== done"
