# A compound literal that follows a nameref to an *element* binds a variable
# whose name is the reference's whole spelling — `n[1]`, brackets and all — and
# never touches the array the text seems to point at. That is
# a-local-declared-through-a-reference-to-an-element-is-named-by-the-spelling's
# rule; this file is about `-g`, which does not turn it off. `-g` only changes
# which binding of that name the literal lands in.
#
# Without `-g` the declaration makes a *local*, so the spelling names a fresh
# one in this frame. With `-g` it binds the name the ordinary way instead: the
# innermost binding already visible under that name, or a new global if there is
# none. So `-g` from a deeper frame overwrites the caller's spelling-named local
# rather than reaching past it.
#
# Which reference is consulted is the usual question of *which binding this
# declaration writes*: `-g` writes the global one, so a `-g` operand follows a
# global `declare -n r='n[1]'` and ignores a local one — a local reference with
# no global of the same name leaves `-g` declaring a plain global `r`.
#
# The scalar half is unmoved: `declare -g r=zz` through the same reference
# stores the element, as it always did.
#
# At top level there is no frame at all and the literal is refused with
# `` `n[1]': not a valid identifier ``, as it always was.

echo '=== -g through a global reference binds the spelling, globally'
( n=(a b c); declare -n r='n[1]'
  f() { declare -g r=(x y); echo "s=$?"; declare -p 'n[1]'; }
  f; echo "after:"; declare -p 'n[1]'; declare -p n; declare -p r )
echo '=== …and any reference spelled the same finds it; the element does not'
( n=(a b c); declare -n r='n[1]'
  f() { declare -g r=(x y); }
  f; declare -n s='n[1]'; echo "s=[${s[*]}]"; echo "n1=[${n[1]}]" )
echo '=== it binds the name, so a caller-s local of that name is what it hits'
( n=(a b c); declare -n r='n[1]'
  g() { declare -g r=(x y); declare -p 'n[1]'; }
  f() { declare -n q='n[1]'; declare -a q=(local one); declare -p 'n[1]'
        g; declare -p 'n[1]'; }
  f; echo "after:"; declare -p 'n[1]' 2>&1 )
echo '=== -g asks the global binding, so a local reference is not the one it follows'
( n=(a b c)
  f() { declare -n r='n[1]'; declare -g r=(x y); echo "s=$?"; declare -p r; }
  f; echo "after:"; declare -p r 2>&1; declare -p n; declare -p 'n[1]' 2>&1 )
( n=(a b c)
  f() { declare -n r='n[1]'; declare -g r=zz; echo "s=$?"; declare -p r; }
  f; echo "after:"; declare -p r 2>&1; declare -p n; declare -p 'n[1]' 2>&1 )
echo '=== the scalar -g through a global reference stores the element, as ever'
( n=(a b c); declare -n r='n[1]'
  f() { declare -g r=zz; echo "s=$?"; }
  f; declare -p n; declare -p 'n[1]' 2>&1 )
echo '=== a plain reference under -g is unaffected'
( n=(a b c); declare -n r=n
  f() { declare -g r=(x y); echo "s=$?"; }
  f; declare -p n )
echo '=== the spelling carries its own kind, unrelated to the base array-s'
( declare -A m=([k]=K); declare -n r='m[k]'
  f() { declare -gA r=([q]=v); echo "s=$?"; declare -p m; declare -p 'm[k]'; }
  f )
( declare -A m=([k]=K); declare -n r='m[k]'
  f() { declare -g r=(x y); echo "s=$?"; declare -p m; declare -p 'm[k]'; }
  f )
echo '=== at top level it is still refused'
( n=(a b c); declare -n r='n[1]'; declare -g r=(x y); echo "s=$?"; declare -p n; declare -p 'n[1]' 2>&1 )
( n=(a b c); declare -n r='n[1]'; declare r=(x y); echo "s=$?"; declare -p n )
