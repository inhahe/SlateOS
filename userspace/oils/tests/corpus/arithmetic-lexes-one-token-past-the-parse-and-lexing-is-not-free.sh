# bash's arithmetic evaluator runs its lexer one token ahead of its parser.
# `subexpr` primes it —
#
#     readtok ();
#     val = EXP_LOWEST ();
#     if (curtok != 0)
#       evalerror (_("syntax error in expression"));
#
# — (expr.c:472-479) and every production that consumes a token calls `readtok`
# again straight away, so the token *after* a finished operand is already lexed
# by the time the parse discovers it has no use for it.
#
# Lexing a token is not free. `readtok` (expr.c:1310-1512) evaluates a NAME the
# moment it forms it (`tokval = expr_streval (tokstr, e, &curlval)`, 1385-1390),
# converts a numeric literal with `strlong`, scans a subscript, and refuses a
# character it has no token for. All of that therefore happens to a leftover the
# parse is about to reject — and, happening first, beats the rejection.
#
# The lexer is one token deeper still. Before it will evaluate a NAME it lexes a
# whole *further* token with `noeval = 1`, purely to see whether a bare `=`
# follows and the name is about to be assigned rather than read:
#
#     SAVETOK (&ec);  …  noeval = 1;  curtok = STR;  readtok ();
#     peektok = curtok;  …  RESTORETOK (&ec);
#     if (lasttok == PREINC || lasttok == PREDEC || peektok != EQ)
#       tokval = expr_streval (tokstr, e, &curlval);
#
# `noeval` suppresses `expr_streval` (1157-1160) and nothing else, so that peek
# can still fail on its own account — and its failure precedes everything the
# name would have done, `set -u` included. `SAVETOK`/`RESTORETOK` (229-254) carry
# `lasttp`, so a peek that succeeds leaves no trace: neither the cursor nor the
# error-token position moves.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== 1. a leftover NAME is evaluated before the parse rejects it"
# `bad` is never an operand — the expression is already complete — yet its value
# is evaluated as arithmetic of its own, and that is the error reported.
e 'bad="1 + "; echo "[$(( 4 bad ))]"'
e 'bad="1 + "; w=" 3 bad "; echo "[$(( $w ))]"'
# Only the *last* operand's follower is reached: one token of lookahead, not two.
e 'b2="2 * "; echo "[$(( 4 zzz b2 ))]"'
# It recurses as any read does, and answers to the same recursion limit.
e 'a=b; b=c; c="1 + "; echo "[$(( 4 a ))]"'
e 'a=b; b=a; echo "[$(( 4 a ))]"'
# An unset name reads as 0 and raises nothing, so the parse's own complaint is
# what comes out — this is the control that isolates the read.
e 'echo "[$(( 4 zzz ))]"'
# …unless `set -u` is on, which the read answers to like any other.
e 'set -u; echo "[$(( 4 zzz ))]"'
# The read is a read: its assignments happen, and outlive the failed expression.
e 'x="y=5"; (( 4 x )); echo "y=[$y] rc=$?"'
e 'x="y=5"; let "4 x"; echo "y=[$y] rc=$?"'
e 'x="y=5"; if (( 4 x )); then :; fi; echo "y=[$y]"'

echo "=== 2. every grammar position ends at the same loop"
# Each of these finishes an operand somewhere different, and every one of them
# reaches the leftover through the binary-operator loop.
e 'bad="1 + "; echo "[$(( (1) bad ))]"'
e 'bad="1 + "; echo "[$(( 1, 2 bad ))]"'
e 'bad="1 + "; echo "[$(( x = 1 bad ))]"'
e 'bad="1 + "; echo "[$(( 1 bad ))]"'
e 'bad="1 + "; echo "[$(( x++ bad ))]"'
e 'bad="1 + "; echo "[$(( !0 bad ))]"'
e 'bad="1 + "; echo "[$(( -1 bad ))]"'
e 'bad="1 + "; echo "[$(( 1 + 2 bad ))]"'
e 'bad="1 + "; echo "[$(( 1 && 2 bad ))]"'
e 'bad="1 + "; a=(1 2); echo "[$(( a[0] bad ))]"'
e 'bad="1 + "; echo "[$(( 4 bad ++ ))]"'
e 'bad="1 + "; echo "[$(( 4 bad , 5 ))]"'
e 'bad="1 + "; echo "[$(( 4 bad ? 1 : 2 ))]"'
# Including inside an unclosed group, whose missing `)` is never reached.
e 'bad="1 + "; echo "[$(( (1 bad) ))]"'

echo "=== 3. lexing runs ahead of evaluating, so it wins the race"
# The lookahead happens as the *previous* operand is consumed, before the
# operator that would have used it is applied.
e 'bad="1 + "; echo "[$(( 1 / 0 bad ))]"'
e 'bad="1 + "; echo "[$(( 2 ** -1 bad ))]"'
# The controls, with nothing to look ahead at.
e 'echo "[$(( 1 / 0 zzz ))]"'
e 'echo "[$(( 2 ** -1 ))]"'

echo "=== 4. a leftover number is converted, and can fail to convert"
e 'echo "[$(( 4 5 ))]"'
e 'echo "[$(( 4 5x ))]"'
e 'echo "[$(( 4 08 ))]"'
e 'echo "[$(( 4 2#3 ))]"'
e 'echo "[$(( 4 99#1 ))]"'
e 'echo "[$(( 4 0x1g ))]"'
e 'echo "[$(( 4 0x ))]"'
# A literal is a maximal run of base-64 digit characters, so the `[` is not
# reached either.
e 'echo "[$(( 4 5x[1] ))]"'
e 'echo "[$(( 4 5x bad ))]"'

echo "=== 5. a leftover subscript is scanned, and an indexed one evaluated"
e 'echo "[$(( 4 zzz[ ))]"'
e 'echo "[$(( 4 zzz[1+] ))]"'
e 'bad="1 + "; a=(9 8); echo "[$(( 4 a[bad] ))]"'
e 'a=(9 8); echo "[$(( 4 a[1] ))]"'

echo "=== 6. a leftover the lexer has no token for is refused where it is met"
e 'echo "[$(( 4 @ ))]"'
e 'echo "[$(( 4 ] ))]"'
e 'echo "[$(( 4 } ))]"'
e 'echo "[$(( 4 [ ))]"'
e 'let "2+3;"'
# A token it *does* have is the parse's complaint instead.
e 'echo "[$(( 4 : ))]"'
e 'echo "[$(( 4 ) ))]"'
e 'let "2+3]"'
e 'let "2+3:"'

echo "=== 7. the lookahead is suppressed by \`noeval\`, the lex is not"
# `&&`, `||` and `?:` raise `noeval` over the operand they have decided against,
# and the leftover inside it is lexed there — so the name is not read…
e 'bad="1 + "; echo "[$(( 0 && 4 bad ))]"'
e 'bad="1 + "; echo "[$(( 1 || 4 bad ))]"'
e 'bad="1 + "; echo "[$(( 1 ? 9 : 4 bad ))]"'
e 'bad="1 + "; echo "[$(( 0 ? 4 bad : 9 ))]"'
e 'set -u; echo "[$(( 0 && 4 zzz ))]"'
# …but the literal is still converted and the stray character still refused.
e 'echo "[$(( 0 && 4 5x ))]"'
e 'echo "[$(( 1 ? 9 : 4 5x ))]"'
e 'echo "[$(( 0 ? 4 5x : 9 ))]"'
e 'echo "[$(( 0 ? 4 @ : 9 ))]"'
e 'echo "[$(( 1 ? 9 : 4 @ ))]"'
e 'echo "[$(( 0 ? 4 a[1+] : 9 ))]"'

echo "=== 8. the lookahead names the error token, even when it does nothing"
# `lasttp` moves to the token the lexer read, so the diagnostic that follows
# names it and not the operand before it.
e 'echo "[$(( 0 ? 4 5 : 9 ))]"'
e 'echo "[$(( 0 ? 4 zzz : 9 ))]"'
e 'echo "[$(( 0 ? 4 ) : 9 ))]"'
# At end of input the lexer reads no token and keeps the last one it did, which
# is what makes these name an operand rather than nothing.
e 'echo "[$(( 2 ** -1 ))]"'
e 'echo "[$(( 2 ** (-1) ))]"'
e 'let "(2+3"'
e 'let "((2+3)"'

echo "=== 9. a NAME's own peek for \`=\`, one token deeper again"
# The peek decides whether the name is read at all: a bare `=` means assignment.
e 'bad="1 + "; echo "[$(( bad = 5 ))]; bad=$bad"'
e 'bad="1 + "; echo "[$(( bad += 5 ))]"'
e 'bad="1 + "; echo "[$(( bad == 5 ))]"'
e 'bad="1 + "; echo "[$(( 4 bad=1 ))]"'
e 'bad="1 + "; echo "[$(( 4 bad==1 ))]"'
e 'bad="1 + "; echo "[$(( 4 bad+=1 ))]"'
# It is a whole token, lexed with evaluation suppressed — so it cannot read a
# name, but it can still fail on a literal or a stray character, and that
# failure precedes the name's own read.
e 'x=1; echo "[$(( x 5x ))]"'
e 'x=1; echo "[$(( x 99#1 ))]"'
e 'x=1; echo "[$(( x 0x1g ))]"'
e 'x=1; echo "[$(( 1 + x 5x ))]"'
e 'bad="1 + "; echo "[$(( bad 5x ))]"'
e 'set -u; echo "[$(( zzz @ ))]"'
e 'set -u; echo "[$(( zzz 5x ))]"'
# Suppressed there too, so a chain of names is walked to its end.
e 'x=1; echo "[$(( x y 5x ))]"'
e 'x=1; echo "[$(( x y z 5x ))]"'
e 'x=1; echo "[$(( x y @ ))]"'
e 'x=1; echo "[$(( 0 && x 5x ))]"'
e 'x=1; echo "[$(( 1 ? 9 : x 5x ))]"'
# And it leaves nothing behind when it succeeds: the name is read as usual.
e 'x=1; echo "[$(( x ))]"'
e 'x=1; y=2; echo "[$(( x + y ))]"'
e 'x=1; echo "[$(( x @ ))]"'

echo "=== 10. \`++\` in operator position is a prefix increment, not two operators"
# The lexer looks forward from a `++` whose left operand cannot take a postfix:
# an assignable name after it makes the token PREINC — which ends the expression
# — and anything else splits it into a binary and a unary sign.
e 'echo "[$(( 4 ++ 5 ))]"'
e 'echo "[$(( 2 -- 3 ))]"'
e 'echo "[$(( 2--3 ))]"'
e 'echo "[$(( 2++3 ))]"'
e 'echo "[$(( 4 ++ y ))]"'
e 'y=7; echo "[$(( 4 ++ y ))]"; echo "y=$y"'
e 'y=7; echo "[$(( 4 -- y ))]"; echo "y=$y"'
e 'y=7; echo "[$(( 4 ++y ))]"'
e 'y=7; echo "[$(( (1) ++ y ))]"; echo "y=$y"'
# The controls: a left operand that *can* take a postfix keeps it.
e 'x=1; y=2; echo "[$(( x ++ y ))]"'
e 'x=1; y=2; echo "[$(( x+++y ))]; x=$x"'
e 'x=1; y=2; echo "[$(( x---y ))]; x=$x"'

echo done
