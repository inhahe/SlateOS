# bash truncates a declaration operand at its first `[` before it does anything
# with it, keeping the bracket text aside only to check that it is well formed.
# So `declare 'r[1]'` is a command about `r`, never about the spelling `r[1]`,
# and three things follow from that:
#
#   - the subscript is never *evaluated* on the valueless path — it only has to
#     be non-empty and properly closed, so it may hold anything, an `=` and a
#     side effect included, and none of it happens;
#   - the `=` that splits `NAME=value` is therefore the one *outside* the
#     subscript, not the first one in the word;
#   - with nothing else asked for, the name that is looked up is the truncated
#     one, through a plain `find_variable` — which follows a reference only as
#     far as a variable that already *exists*, and otherwise answers nothing at
#     all, leaving the array to be made of the operand's own name.
#
# Any letter that asks something of a variable puts the operand back on the
# branch that builds a name out of the reference's value, where a target's own
# subscript and the operand's cannot both stand and the doubled name is refused.
# `-I` asks nothing and stays; `-G` is `-g`'s twin and chooses a binding, so it
# goes.

echo '=== with nothing else asked for, an element reference makes the array of the reference'
( n=(a b c); declare -n r='n[1]';  declare 'r[1]'; echo "rc=$?"; declare -p n r )
( n=(a b c); declare -n r='n[@]';  declare 'r[1]'; echo "rc=$?"; declare -p n r )
( n=(a b c); declare -n r='n[1+]'; declare 'r[1]'; echo "rc=$?"; declare -p n r )
( declare -A m=([k]=v); declare -n r='m[k]'; declare 'r[1]'; echo "rc=$?"; declare -p m r )

echo '=== and so does a reference to a name nothing has bound'
( declare -n r=v; declare 'r[1]'; echo "rc=$?"; declare -p r; declare -p v )

echo '=== a reference to one that exists makes the array of the target instead'
( s=hi; declare -n r=s; declare 'r[1]'; echo "rc=$?"; declare -p r s )
( z=(a b); declare -n r=z; declare 'r[1]'; echo "rc=$?"; declare -p r z )
( s=hi; declare -n r2=s; declare -n r=r2; declare 'r[1]'; echo "rc=$?"; declare -p r r2 s )
( declare -A m=([k]=v); declare -n r=m; declare 'r[1]'; echo "rc=$?"; declare -p r m )
( declare -ra z=(a b); declare -n r=z; declare 'r[1]'; echo "rc=$?"; declare -p z )

echo '=== a circular chain has no target either, and says so first'
( declare -n a=b; declare -n b=a; declare 'a[1]'; echo "rc=$?"; declare -p a b )

echo '=== the array is made invisible, and an existing one is left exactly as it was'
( declare 'z[1]'; echo "rc=$?"; declare -p z )
( z=(a b c); declare 'z[1]'; echo "rc=$?"; declare -p z; echo "len=${#z[@]}" )
( s=hi; declare 's[1]'; echo "rc=$?"; declare -p s )
( declare -i s=5; declare 's[1]'; echo "rc=$?"; declare -p s )
( readonly s=hi; declare 's[1]'; echo "rc=$?"; declare -p s )
( declare -A m=([k]=v); declare 'm[k]'; echo "rc=$?"; declare -p m )

echo '=== the subscript is not evaluated: anything non-empty goes, and nothing happens'
( declare 'z[1+]';   echo "rc=$?"; declare -p z )
( declare 'z[@]';    echo "rc=$?"; declare -p z )
( declare 'z[-1]';   echo "rc=$?"; declare -p z )
( declare 'z[a[0]]'; echo "rc=$?"; declare -p z )
( declare 'z[x=7]';  echo "rc=$? x=[$x]"; declare -p z )
( z=(a b c); declare 'z[x=1]'; echo "rc=$? x=[$x]"; declare -p z )

echo '=== so the assignment split is the = outside the subscript'
( declare 'z[x=7]=v'; echo "rc=$? x=[$x]"; declare -p z )
( z=(a b c); declare 'z[x=1]+=Q'; echo "rc=$? x=[$x]"; declare -p z )
( declare -A m; declare 'm[a=b]=v'; echo "rc=$?"; declare -p m )
( f(){ declare -A m; local 'm[a=b]=v'; echo "rc=$?"; declare -p m; }; f )
( declare 'z=1=2'; echo "rc=$?"; declare -p z )

echo '=== an empty subscript is no subscript, and is refused as the identifier it is not'
( declare 'z[]';                  echo "rc=$?"; declare -p z )
( z=(a b c); declare 'z[]';       echo "rc=$?"; declare -p z )
( declare -A m=([k]=v); declare 'm[]'; echo "rc=$?"; declare -p m )
( s=hi; declare 's[]';            echo "rc=$?"; declare -p s )
( declare -i 'z[]';               echo "rc=$?"; declare -p z )
( n=(a b c); declare -n r='n[1]'; declare 'r[]'; echo "rc=$?"; declare -p r )
( s=hi; declare -n r=s; declare 'r[]'; echo "rc=$?"; declare -p r s )

echo '=== a valued one has a store to attempt, and is refused by the store instead'
( z=(a b); declare 'z[]=v'; echo "rc=$?"; declare -p z )
( declare -A m; declare 'm[]=v'; echo "rc=$?"; declare -p m )

echo '=== a bracket that never closes, or that is not the last thing, is no name at all'
( declare 'z[1';     echo "rc=$?"; declare -p z )
( declare 'z[1]junk'; echo "rc=$?"; declare -p z )
( declare 'z[1]]';   echo "rc=$?"; declare -p z )
( declare '1x[1]';   echo "rc=$?" )

echo '=== a letter that asks for something puts the doubled name back'
( n=(a b c); declare -n r='n[1]'; declare -i 'r[1]'; echo "rc=$?" )
( n=(a b c); declare -n r='n[1]'; declare -r 'r[1]'; echo "rc=$?" )
( n=(a b c); declare -n r='n[1]'; declare +l 'r[1]'; echo "rc=$?" )
( n=(a b c); declare -n r='n[1]'; declare -g 'r[1]'; echo "rc=$?" )
( n=(a b c); declare -n r='n[1]'; declare -G 'r[1]'; echo "rc=$?" )

echo '=== -I asks nothing, so it stays where the bare form is'
( n=(a b c); declare -n r='n[1]'; declare -I 'r[1]'; echo "rc=$?"; declare -p r )
( declare -n r=v; declare -I 'r[1]'; echo "rc=$?"; declare -p r; declare -p v )
( declare -I 'z[1]'; echo "rc=$?"; declare -p z )

echo '=== a listing is a listing, and a local is named by the whole spelling'
( n=(a b c); declare -n r='n[1]'; declare -p 'r[1]'; echo "rc=$?" )
( f(){ local 'z[1]'; echo "rc=$?"; declare -p z; }; f )
( n=(a b c); f(){ declare -n r='n[1]'; local 'r[1]'; echo "rc=$?"; declare -p r; }; f )

echo '=== a value takes the reference off the operand, as it always did'
( n=(a b c); declare -n r='n[1]'; declare 'r[1]'=v; echo "rc=$?"; declare -p n r )

echo '=== done'
