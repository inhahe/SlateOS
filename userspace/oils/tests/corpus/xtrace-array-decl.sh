# How `set -x` traces an array literal that is an *operand* of a declaration
# builtin (`declare -a x=(1 2)`, `readonly -a r=(…)`, `local -A m=(…)`).
#
# bash performs such a compound assignment during the command's word-expansion
# pass, not inside the builtin — only a compound can bind a whole array — so the
# trace reflects that split:
#
#   * one `name=(…)` line per compound operand, in operand order, all *before*
#     the builtin's own line;
#   * the elements shown are the fully expanded *fields* (split, globbed,
#     brace-expanded), each wrapped in single quotes unconditionally — unlike a
#     command word, which bash quotes only when it has to;
#   * a keyed element renders as `['idx']='val'` with the subscript's
#     post-expansion, pre-arithmetic, untrimmed text;
#   * the builtin's line shows a bare *name* where each compound operand was
#     written, at that operand's original position among the words.
#
# Everything runs in a subshell with `2>&1` so the trace lands on stdout in a
# fixed order instead of racing the real output.

# --- the basic shapes: several elements, an empty element, an empty literal.
( set -x; declare -a b=(x "") ) 2>&1
( set -x; declare -a e=() ) 2>&1
( set -x; declare -a one=(solo) ) 2>&1

# --- quoting is unconditional, so even a plain word is quoted; an embedded
# single quote becomes '\'' and a control character is left raw (no $'…').
( set -x; declare -a q=(plain "it's" $'a\tb') ) 2>&1
( set -x; declare -a c=($'a\002b' $'a\177b' $'a\033b') ) 2>&1
# (`$'\001'` is deliberately absent: bash doubles it in this one trace shape —
# 0x01 is its internal CTLESC and the compound-assignment printer forgets to
# strip it — while the stored value is fine. osh prints the byte once. See
# TD-OILS-XTRACE-CTLESC-LEAK.)

# --- append keeps its `+=`.
( set -x; declare -a p=(1); declare -a p+=(2) ) 2>&1

# --- an associative literal in subscript mode: the subscript text is traced as
# written after expansion, *before* the arithmetic runs and without trimming.
( set -x; declare -A m=([k]=v [1+1]=w) ) 2>&1
( set -x; i=2; declare -a s=([i]=q [$i]=r) ) 2>&1
( set -x; declare -a sp=([ 1 ]=v [ i+1 ]=w) ) 2>&1
( set -x; declare -a rev=([2]=c [0]=a) ) 2>&1

# --- an associative literal in *pair* mode (first element unsubscripted): each
# flattened field is one traced element, and a keyed element among them is
# traced as the reassembled `[a]=1` word that pair mode treats it as.
( declare -A pm; set -x; declare -A pm=(k1 v1 k2 v2) ) 2>&1
( declare -A pm; set -x; declare -A pm=(x y [a]=1 z) ) 2>&1

# --- the value-transforming attributes are in force *before* the literal binds,
# but the trace shows the pre-transform field: `-i` traces `1+1`, not `2`.
( set -x; declare -ai n=(1+1 "3 * 2") ) 2>&1
( set -x; declare -al f=(AB Cd) ) 2>&1
( set -x; declare -au u=(ab) ) 2>&1

# --- expansions are traced by their result: word splitting, globbing and brace
# expansion have all already happened, so one field per resulting word.
( set -x; two="a b"; declare -a h=($two "$two") ) 2>&1
( set -x; ev=; declare -a v=($ev) ) 2>&1
( set -x; ev=; declare -a w=("$ev") ) 2>&1
( set -x; declare -a br=({1..3}) ) 2>&1
( set -x; declare -a nl=($'x\ny') ) 2>&1
( set -x; declare -a bs=('a\b' "c\d") ) 2>&1
( set -x; declare -a df=(${!nope-def}) ) 2>&1
# A glob that matches nothing stays literal (no `failglob` here).
( cd "$TMPDIR" 2>/dev/null || cd /tmp; set -x; declare -a g=(*.nosuchglob) ) 2>&1

# --- a command substitution inside the literal traces its own `++` line first,
# and exactly once: the operand's line is rendered from the same expansion.
( set -x; declare -a cs=($(echo z)) ) 2>&1
( set -x; declare -a cs2=($(echo "a b")) ) 2>&1
( set -x; declare -A ck=([$(echo k)]=$(echo v)) ) 2>&1

# --- the builtin's own line: a bare name stands in for the compound operand at
# its source position, so a scalar operand on either side keeps its place.
( set -x; declare -x SC=1 arr=(9) SD=2 ) 2>&1
( set -x; declare -a x=(1 2) y=(3) ) 2>&1
( set -x; declare -a p q=(1) r s=(2) t ) 2>&1
( set -x; declare -a tail=(1) plain ) 2>&1

# --- `readonly`/`export`/`local` take the same path.
( set -x; readonly -a ro=(5) ) 2>&1
( set -x; export -a ex=(6) ) 2>&1
( set -x; f() { local -a lo=(7 8); }; f ) 2>&1
( set -x; f() { local -A lm=([k]=v); }; f ) 2>&1

# --- PS4 applies to the operand lines too, and a multi-character PS4 repeats
# only its first character for nesting depth.
( PS4='T '; set -x; declare -a ps=(1) ) 2>&1
( PS4='T '; set -x; declare -a ps=($(echo 1)) ) 2>&1
