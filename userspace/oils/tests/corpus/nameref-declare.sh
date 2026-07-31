# The declaration builtins declare what a reference *points at*.
#
# `declare -n r=w` makes `r` a reference to `w`. Every later declaration naming
# `r` — an attribute, a kind, a value — is resolved through the reference first
# and applied to `w`, and `r` is left the reference it was:
#
#   w=5; declare -n r=w; declare -i r    →   declare -i w="5"
#                                            declare -n r="w"
#
# The exemptions are the declarations that are *about* the reference: `-n`
# (declaring or retargeting it), `+n` (taking the attribute away), `declare -p`
# (which reports the reference itself) and `unset -n`. A bare `unset r` is not
# among them — it unsets the target.
#
# Two more things decide it. Which *binding* the declaration writes: a
# local-binding `declare`/`local` writes the current frame's, so a reference
# further out is merely what it shadows. And whether the operand carries a
# subscript: an element assignment needs an array to store into, and where that
# array can only be the reference's own binding the reference is dropped
# instead of followed.
#
# `export` and `readonly` follow too, and have no exemption at all: neither has
# a `-n` operand of its own, and `export -n` (which removes the export
# attribute) removes it from the target.
#
# Two chains name nothing to declare, and each has its own answer. A circular
# one warns and drops the operand, leaving the status alone — except that a
# value the operand carried has nowhere to go, which is a failure. An element
# reference (`declare -n r=arr[1]`) names no *variable*: `declare` puts the
# attributes on `arr` and the value in `arr[1]`, but `export`/`readonly` refuse
# a subscript outright — while still storing through the reference.
#
# bash emits its circular-nameref warning a varying number of times (see
# known-issues TD-OILS-NAMEREF-WARNING-COUNT), so the cases below that provoke
# one either drop the warning or show it deduplicated.
d() { grep -v 'circular name reference'; }
u() { grep 'circular name reference' | sort -u; }

echo "=== which declarations follow the reference"
( w=5; declare -n r=w; declare -i r;   declare -p w r ) 2>&1
( w=5; declare -n r=w; typeset -x r;   declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -u r;   declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -t r;   declare -p w r ) 2>&1
( w=5; declare -n r=w; declare r;      declare -p w r ) 2>&1
( w=5; declare -n r=w; readonly r;     declare -p w r ) 2>&1
( w=5; declare -n r=w; export r;       declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -a r;   declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -A r;   declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -i r=9; declare -p w r ) 2>&1
( w=5; declare -n r=w; export r=9;     declare -p w r ) 2>&1
( w=5; declare -n r=w; readonly r=9;   declare -p w r ) 2>&1
( w=5; declare -n r=w; export -a r=9;    declare -p w r ) 2>&1
( w=5; declare -n r=w; readonly -a r=9;  declare -p w r ) 2>&1
# `export -n` takes the attribute off the *target*, so `w` stops being exported.
( w=5; declare -n r=w; export w; export -n r; declare -p w r ) 2>&1

echo "=== …and which are about the reference itself"
( w=5; declare -n r=w; declare -p r ) 2>&1
( w=5; declare -n r=w; declare -n r; declare -p w r ) 2>&1
( w=5; declare -n r=w; declare +n r; declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -n r=w2; w2=9; declare -p w r w2 ) 2>&1
( w=5; declare -n r=w; unset -n r; declare -p w r ) 2>&1
( w=5; declare -n r=w; unset r; declare -p w r ) 2>&1

echo "=== the target does not have to exist yet"
( declare -n r=nope; declare -i r;   declare -p nope r ) 2>&1
( declare -n r=nope; declare -i r=5; declare -p nope r ) 2>&1
( declare -n r=nope; declare -a r;   declare -p nope r ) 2>&1
( declare -n r=nope; export r=5;     declare -p nope r ) 2>&1
( declare -n r=nope; export r;       declare -p nope r ) 2>&1
( declare -n r=nope; readonly r=5;   declare -p nope r ) 2>&1

echo "=== a chain is followed the whole way"
( w=5; declare -n a=w; declare -n b=a; declare -i b; declare -p w a b ) 2>&1
( w=5; declare -n a=w; declare -n b=a; export b;     declare -p w a b ) 2>&1

echo "=== an element reference"
# `declare` marks the array and stores into the element.
( declare -a arr=(1 2); declare -n r=arr[1]; declare -i r;    declare -p arr r ) 2>&1
( declare -a arr=(1 2); declare -n r=arr[1]; declare -u r;    declare -p arr r ) 2>&1
( declare -a arr=(1 2); declare -n r=arr[1]; declare r=zz;    declare -p arr r ) 2>&1
( declare -A m=([k]=1); declare -n r=m[k];   declare -i r=3+4; declare -p m r ) 2>&1
# `export`/`readonly` refuse the subscript — but the store still happens, and
# the status is still 0.
( declare -a arr=(1 2); declare -n r=arr[1]; export r;    echo "rc=$?"; declare -p arr r ) 2>&1
( declare -a arr=(1 2); declare -n r=arr[1]; readonly r;  echo "rc=$?"; declare -p arr r ) 2>&1
( declare -a arr=(1 2); declare -n r=arr[1]; export r=9;  echo "rc=$?"; declare -p arr ) 2>&1
( declare -a arr=(1 2); declare -n r=arr[1]; export r+=9; echo "rc=$?"; declare -p arr ) 2>&1
( declare -A m=([k]=1); declare -n r=m[k];   export r=9;  echo "rc=$?"; declare -p m ) 2>&1
# …and the refused `readonly` really did not take, so the element still writes.
( declare -a arr=(1 2); declare -n r=arr[1]; readonly r; arr[1]=x; declare -p arr ) 2>&1

echo "=== a circular chain declares nothing"
# The operand is dropped and the status left alone — unless it carried a value,
# which then has nowhere to go, and *that* is the failure.
( declare -n a=b; declare -n b=a; declare -i a; echo "rc=$?"; declare -p a b ) 2>&1 | d
( declare -n a=b; declare -n b=a; export a;     echo "rc=$?"; declare -p a b ) 2>&1 | d
( declare -n a=b; declare -n b=a; readonly a;   echo "rc=$?" ) 2>&1 | d
( declare -n a=b; declare -n b=a; export a=5;   echo "rc=$?"; declare -p a b ) 2>&1 | d
( declare -n a=b; declare -n b=a; readonly a=5; echo "rc=$?" ) 2>&1 | d
# …and it does say so, however many times over.
( declare -n a=b; declare -n b=a; declare -i a ) 2>&1 | u
( declare -n a=b; declare -n b=a; export a )     2>&1 | u
( declare -n a=b; declare -n b=a; export a=5 )   2>&1 | u

echo "=== a self reference never becomes one, so nothing is followed"
( declare -n r=r; declare -i r; declare -p r ) 2>&1
( declare -n r=r; export r;     declare -p r ) 2>&1

echo "=== a subscripted operand needs an array, and that decides what it declares"
# An element assignment has to have an array to store into. Where the
# declaration writes the reference's own binding, that array can only be the
# operand's, so the reference is dropped — with a warning — and the operand
# declares itself. A valueless operand stores nothing and resolves as ever.
( w=5; declare -n r=w; declare -i r[1];    echo "rc=$?"; declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -A r[k];    echo "rc=$?"; declare -p w r ) 2>&1
( declare -a arr=(1 2); declare -n r=arr; declare -i r[1]; declare -p arr r ) 2>&1
( w=5; declare -n r=w; declare r[1]=9;     echo "rc=$?"; declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -i r[1]=3+4; echo "rc=$?"; declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -A r[k]=9;  echo "rc=$?"; declare -p w r ) 2>&1
( w=5; declare -n r=w; declare r[1]+=9;    echo "rc=$?"; declare -p w r ) 2>&1
( declare -a arr=(1 2); declare -n r=arr;    declare r[1]=9; declare -p arr r ) 2>&1
( declare -a arr=(1 2); declare -n r=arr[1]; declare r[0]=9; declare -p arr r ) 2>&1
( declare -A m=([k]=1); declare -n r=m[k];   declare r[0]=9; declare -p m r ) 2>&1
# A target that does not exist yet is still where a valueless one lands.
( declare -n r=nope; declare -i r[1]; echo "rc=$?"; declare -p nope r ) 2>&1
# Two subscripts cannot stand: nothing is *named* `arr[1]`, so `arr[1][0]` is
# no identifier either, and the operand declares nothing.
( declare -a arr=(1 2); declare -n r=arr[1]; declare -i r[0]; echo "rc=$?"; declare -p arr r ) 2>&1
( declare -A m=([k]=1); declare -n r=m[k];   declare -i r[0]; echo "rc=$?"; declare -p m r ) 2>&1
# A circular chain has no target to resolve to, so the array it makes is the
# operand's own — as the valued form's is.
( declare -n a=b; declare -n b=a; declare a[1]=9;  echo "rc=$?"; declare -p a b ) 2>&1 | d
( declare -n a=b; declare -n b=a; declare -i a[1]; echo "rc=$?"; declare -p a b ) 2>&1 | d
# A declaration that binds a *local* array builds it from the resolved name
# instead, so both forms follow there.
( w=5; f() { local -n r=w; local r[1]=9;   declare -p r w; }; f; declare -p w ) 2>&1
( w=5; f() { local -n r=w; local -i r[1];  declare -p r w; }; f; declare -p w ) 2>&1
# …while a global reference the frame is only shadowing is not followed at all,
# so no reference is dropped and nothing is warned about.
( w=5; declare -n r=w; f() { declare r[1]=9;  declare -p r; }; f; declare -p w r ) 2>&1
( w=5; declare -n r=w; f() { declare -i r[1]; declare -p r; }; f; declare -p w r ) 2>&1

echo "=== a reference is a name, so a -n operand may not carry a subscript"
( w=5; declare -n r=w; declare -n r[1]=w; echo "rc=$?"; declare -p w r ) 2>&1
( w=5; declare -n r=w; declare -n r[1];   echo "rc=$?"; declare -p w r ) 2>&1
( declare -n zz[1]=w;  echo "rc=$?" ) 2>&1
( f() { local -n zz[1]=w; echo "rc=$?"; }; f ) 2>&1

echo "=== export and readonly do not resolve a subscripted operand at all"
( w=5; declare -n r=w; export 'r[1]=9';   declare -p w r ) 2>&1
( w=5; declare -n r=w; readonly 'r[1]=9'; declare -p w r ) 2>&1

echo "=== readonly through a reference names the target when it refuses"
( w=5; declare -n r=w; readonly r; w=9; echo unreachable ) 2>&1
( w=5; declare -n r=w; readonly r; r=9; echo unreachable ) 2>&1

echo "=== the listings show the target, not the reference"
( w=5; declare -n r=w; declare -i r; declare -i | grep -E ' (w|r)=' ) 2>&1
( w=5; declare -n r=w; export r;     export   | grep -E ' (w|r)=' ) 2>&1
( w=5; declare -n r=w; readonly r;   readonly | grep -E ' (w|r)=' ) 2>&1
( w=5; declare -n r=w; declare -n | grep -E ' r=' ) 2>&1

echo "=== local -n follows the same way"
( w=5; f() { local -n r=w; declare -i r; }; f; declare -p w ) 2>&1
( w=5; f() { local -n r=w; export r; };    f; declare -p w ) 2>&1

# What decides whether a declaration follows a reference is *which binding it
# writes*, not what the name resolves to from where it stands. A declaration
# that binds a local writes the current frame's binding, so a reference of the
# same name further out is merely the thing it shadows: the function gets a
# fresh, ordinary local and the target is untouched. Once the frame's own
# binding is a reference, declarations in that frame follow it as anywhere else.
echo "=== a local-binding declaration writes the frame, so it shadows a reference"
( w=5; declare -n r=w; f() { declare r=9;  declare -p r; }; f; declare -p w r ) 2>&1
( w=5; declare -n r=w; f() { local r=9;    declare -p r; }; f; declare -p w r ) 2>&1
( w=5; declare -n r=w; f() { declare -i r; declare -p r; }; f; declare -p w r ) 2>&1
( w=5; declare -n r=w; f() { local r;      declare -p r; }; f; declare -p w r ) 2>&1
( w=5; declare -n r=w; f() { declare -a r=(1 2); declare -p r; }; f; declare -p w r ) 2>&1
# A second declaration in the same frame adds to the local it already made.
( w=5; declare -n r=w; f() { declare r=9; declare r=7; declare -p r; }; f; declare -p w ) 2>&1
# …and once the frame's binding *is* a reference, the next one follows it —
# into a local of the target, since that declaration binds locally too.
( w=5; f() { local -n r=w; local r=9;      declare -p r w; }; f; declare -p w ) 2>&1
( w=5; f() { local -n r=w; local -i r=3+4; declare -p r w; }; f; declare -p w ) 2>&1
# A caller's local reference is not this frame's binding either.
( w=5; o() { local -n r=w; i; declare -p r; }
       i() { declare r=9; declare -p r; }; o; declare -p w r ) 2>&1

echo "=== -g writes the global binding, and that is the one consulted"
# The global is a reference, so it is followed even from behind a local shadow.
( w=5; declare -n r=w; f() { local r=1; declare -g r=9; declare -p r; }; f; declare -p w r ) 2>&1
# Here it is the *local* that is the reference, and the global is not one.
( w=5; f() { local -n r=w; declare -g r=9; declare -p r; }; f; declare -p w r ) 2>&1

echo "=== export and readonly bind no local, so they always follow"
( w=5; declare -n r=w; f() { export r=9;   }; f; declare -p w r ) 2>&1
( w=5; declare -n r=w; f() { readonly r=9; }; f; declare -p w r ) 2>&1
