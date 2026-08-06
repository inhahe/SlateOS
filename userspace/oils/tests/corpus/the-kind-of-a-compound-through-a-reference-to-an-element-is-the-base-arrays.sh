# `-a` and `-A` name a *kind*, and where a compound literal follows a nameref to
# an element the kind is decided against the array the reference points into —
# the **base**, unsubscripted — and not against the name the literal binds. So
# the refusal a script sees blames `n`, not `n[1]`, and it comes before the
# `` `n[1]': not a valid identifier `` refusal, which is why a whole-array
# reference gives the conversion error alone.
#
# The reach is bash's `find_or_make_array_variable`, so it is not only a
# question: a base that is a *scalar*, or that is not there at all, is **made**
# an array of the literal's kind, the scalar's value carried in as element `0`.
# Only an existing array of the other kind earns the refusal. And it is only the
# kind that reaches the base — no other attribute of the command lands on it.
#
# It happens wherever the declaration would not bind a local: at top level,
# where the operand is then refused as an identifier anyway, and under `-g`,
# where the literal goes on to bind the spelling and both survive. Inside a
# function *without* `-g` the base is never reached at all, because the literal
# binds a local named by the whole spelling and has no business with the array.
#
# The refusal takes the two shapes of
# a-compound-kind-refusal-inside-a-function-is-reported-twice: one untagged line
# and a discarded parse unit at top level, two lines and a mere failure under
# `-g`.

echo '=== the kind is decided against the base array'
( n=(a b c); declare -n r='n[1]'; declare -A r=([k]=v); echo "s=$?"; declare -p n )
( declare -A m=([k]=K); declare -n r='m[k]'; declare -a r=(x y); echo "s=$?"; declare -p m )
echo '=== …and says nothing when the kinds already agree'
( n=(a b c); declare -n r='n[1]'; declare -a r=(x y); echo "s=$?"; declare -p n )
( declare -A m=([k]=K); declare -n r='m[k]'; declare -A r=([q]=v); echo "s=$?"; declare -p m )
echo '=== a base that does not exist has no kind to convert'
( declare -n r='nosuch[1]'; declare -A r=([k]=v); echo "s=$?"; declare -p nosuch 2>&1 )
echo '=== it comes before the bad-subscript line'
( n=(a b c); declare -n r='n[@]'; declare -A r=([k]=v); echo "s=$?" )
( declare -A m=([k]=K); declare -n r='m[@]'; declare -a r=(x y); echo "s=$?" )
( declare -A m=([k]=K); declare -n r='m[@]'; declare -A r=([q]=v); echo "s=$?" )
echo '=== under -g the base is reached and the spelling still binds'
( n=(a b c); declare -n r='n[1]'
  f() { declare -gA r=([k]=v); echo "s=$?"; declare -p n; declare -p 'n[1]' 2>&1; }
  f )
( declare -A m=([k]=K); declare -n r='m[k]'
  f() { declare -ga r=(x y); echo "s=$?"; declare -p m; declare -p 'm[k]' 2>&1; }
  f )
( declare -A m=([k]=K); declare -n r='m[k]'
  f() { declare -gA r=([q]=v); echo "s=$?"; declare -p m; declare -p 'm[k]'; }
  f )
echo '=== a base that is not an array is made one, in the literal-s kind'
( declare -n r='nosuch[1]'
  f() { declare -gA r=([k]=v); echo "s=$?"; declare -p nosuch; declare -p 'nosuch[1]'; }
  f; declare -p nosuch )
( declare -n r='nos2[1]'
  f() { declare -g r=(x y); echo "s=$?"; declare -p nos2; declare -p 'nos2[1]'; }
  f )
( n=hello; declare -n r='n[1]'
  f() { declare -gA r=([k]=v); echo "s=$?"; declare -p n; declare -p 'n[1]'; }
  f )
( n=hello; declare -n r='n[1]'
  f() { declare -g r=(x y); echo "s=$?"; declare -p n; declare -p 'n[1]'; }
  f )
( declare u; declare -n r='u[1]'
  f() { declare -gA r=([k]=v); echo "s=$?"; declare -p u; declare -p 'u[1]'; }
  f )
echo '=== …and a base that is already an array of the same kind is left alone'
( n=(a b c); declare -n r='n[1]'
  f() { declare -ga r=(x y); echo "s=$?"; declare -p n; declare -p 'n[1]'; }
  f )
( declare -A m=([k]=K); declare -n r='m[k]'
  f() { declare -g r=(x y); echo "s=$?"; declare -p m; declare -p 'm[k]'; }
  f )
echo '=== only the kind reaches the base; the other attributes stay on the spelling'
( declare -n r='nos3[1]'
  f() { declare -gArx r=([k]=v); echo "s=$?"; declare -p nos3; declare -p 'nos3[1]'; }
  f )
echo '=== a whole-array reference reaches it the same way'
( declare -n r='nos4[@]'
  f() { declare -gA r=([k]=v); echo "s=$?"; declare -p nos4; declare -p 'nos4[@]'; }
  f )
( declare -A m=([k]=K); declare -n r='m[@]'
  f() { declare -gA r=([q]=v); echo "s=$?"; declare -p m; declare -p 'm[@]'; }
  f )
echo '=== inside a function without -g there is no base to reach'
( n=(a b c)
  f() { declare -n r='n[1]'; declare -A r=([k]=v); echo "s=$?"; declare -p 'n[1]'; declare -p n; }
  f )
( n=(a b c)
  f() { declare -n r='n[@]'; declare -A r=([k]=v); echo "s=$?"; declare -p 'n[@]'; declare -p n; }
  f )
( f() { declare -n r='nosuch[1]'; declare -A r=([k]=v); echo "s=$?"; declare -p 'nosuch[1]'; declare -p nosuch 2>&1; }
  f )
