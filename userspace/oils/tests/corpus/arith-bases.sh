# Arithmetic evaluation beyond the basics covered by arith.sh: literal bases,
# the assignment and comma operators, variable references inside `$(( ))`, and
# the error cases.

# Literal bases. A leading 0 is octal, 0x/0X is hex, and base#digits is any
# base from 2 to 64 (digits 0-9 a-z A-Z @ _, in that order past 9).
echo "$(( 010 )) $(( 0x1f )) $(( 0X1F )) $(( 2#1011 )) $(( 8#777 )) $(( 16#ff )) $(( 36#z )) $(( 64#a )) $(( 64#A )) $(( 64#@ )) $(( 64#_ ))"

# Inside $(( )) a bare name is a variable reference — no $ needed — and an
# unset or non-numeric-but-empty variable reads as 0.
x=6; y=7
echo "$(( x * y )) $(( $x * $y ))"
unset -v nope
echo "$(( nope + 1 ))"
blank=''
echo "$(( blank + 2 ))"

# A variable whose value is itself an expression is evaluated recursively.
expr='x + y'
echo "$(( expr ))"

# Assignment operators write back to the shell variable.
n=10
echo "$(( n += 5 )) n=$n"
echo "$(( n -= 3 )) n=$n"
echo "$(( n *= 2 )) n=$n"
echo "$(( n /= 4 )) n=$n"
echo "$(( n %= 4 )) n=$n"
echo "$(( n <<= 3 )) n=$n"
echo "$(( n >>= 1 )) n=$n"
echo "$(( n |= 1 )) n=$n"
echo "$(( n &= 6 )) n=$n"
echo "$(( n ^= 3 )) n=$n"
echo "$(( n = 42 )) n=$n"

# Pre/post increment differ in the value they yield.
i=5
echo "post=$(( i++ )) now=$i"
i=5
echo "pre=$(( ++i )) now=$i"
i=5
echo "postdec=$(( i-- )) now=$i"

# The comma operator evaluates left to right and yields the last value.
echo "$(( 1, 2, 3 ))"
a=0
echo "$(( a = 1, a + 1 ))"

# Logical operators short-circuit, so the right side's side effect is skipped.
s=0
echo "$(( 0 && (s = 1) )) s=$s"
s=0
echo "$(( 1 || (s = 1) )) s=$s"
s=0
echo "$(( 1 && (s = 1) )) s=$s"

# Unary operators and precedence.
echo "$(( -5 + +3 )) $(( ! 0 )) $(( ! 5 )) $(( ~0 )) $(( ~5 ))"
echo "$(( 2 + 3 * 4 )) $(( (2 + 3) * 4 )) $(( 2 ** 3 ** 2 ))"

# Integers are signed 64-bit and wrap on overflow.
echo "$(( 9223372036854775807 + 1 ))"
echo "$(( -9223372036854775807 - 2 ))"

# Division truncates toward zero, and the sign of % follows the dividend.
echo "$(( -7 / 2 )) $(( 7 / -2 )) $(( -7 % 2 )) $(( 7 % -2 ))"

# `let` and `(( ))` evaluate the same expressions but report a *status*: 0 when
# the last value is nonzero, 1 when it is zero.
let 'v = 3 + 4'; echo "let-status=$? v=$v"
let 'v = 0'; echo "let-zero-status=$? v=$v"
(( 5 > 4 )); echo "arith-cmd-true=$?"
(( 5 < 4 )); echo "arith-cmd-false=$?"

# Arithmetic contexts other than $(( )): array subscripts and the substring
# offsets of ${var:off:len}.
arr=(zero one two three)
k=2
echo "${arr[k]} ${arr[k+1]} ${arr[$(( k - 2 ))]}"
str=abcdefgh
echo "${str:k:k+1}"
echo "${str: -3}"

# Arithmetic errors come in two flavours, and the difference is only visible
# once you look at what still runs afterwards.
#
# A division by zero inside an *expansion* abandons the rest of the logical
# line — `div-zero-status` below never prints — but the script continues at the
# next line.
echo before-div-zero
echo "$(( 1 / 0 ))"; echo "div-zero-status=$?"
echo after-div-zero
# The same division as an arithmetic *command* is merely a failed command: it
# reports on stderr and yields status 1, and the rest of its line runs.
(( 1 % 0 )); echo "mod-zero-status=$?"
# A *syntactically* malformed expression, by contrast, is fatal to a
# non-interactive shell — so this must be last.
echo "$(( 1 + ))"
echo unreachable
