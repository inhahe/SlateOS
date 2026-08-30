# A declaration builtin that binds a **local** through a nameref the current
# frame holds does not store into the array the reference designates. It hands
# the reference's target to bash's `make_local_variable`, which never looks for
# a subscript — so the frame gets a variable whose *name* is the whole spelling,
# `n[1]` brackets and all, and the array `n` is not touched at all.
#
# The spelling is taken verbatim and never parsed: `n[1+1]`, `n[ 1 ]`, `m[k]`
# and `n[@]` are all just names. Every shape of the operand declares that name —
# a value binds it, a valueless one leaves it created-but-unset with whatever
# attributes were asked for — and `declare -p 'n[1]'` prints it back.
#
# The binding is real and frame-scoped: any nameref whose value is that same
# spelling finds it, from this frame or a deeper one, and a write through such a
# reference lands on it. What does *not* see it is a plain `${n[1]}`, which is
# still the array's own element. When the frame returns the name goes with it.
#
# Only a **local** binding does this. At top level, and under `-g`, a scalar
# store follows the reference to the element as it always has, and a compound
# literal there is refused with `` `n[1]': not a valid identifier ``.

echo '=== the value lands in a local named by the spelling, not in the array'
( n=(a b c)
  f() { declare -n r='n[1]'; declare r=zz
        echo "r=[$r]"; echo "n1=[${n[1]}]"; declare -p n; declare -p 'n[1]' 2>&1
        set | grep -a '^n'; }
  f; echo "after: ${n[*]}"; declare -p 'n[1]' 2>&1 )
echo '=== `local` says the same thing'
( n=(a b c)
  f() { local -n r='n[1]'; local r=zz; declare -p 'n[1]' 2>&1; declare -p n; }
  f )
echo '=== a compound literal binds the same name, as an array'
( n=(a b c)
  f() { declare -n r='n[1]'; declare r=(x y); echo "s=$?"
        echo "r=[$r]"; echo "rstar=[${r[*]}]"; declare -p 'n[1]' 2>&1; declare -p n; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; local -a r=(x y); echo "s=$?"; declare -p 'n[1]' 2>&1; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; declare -A r=([k]=v); echo "s=$?"; declare -p 'n[1]' 2>&1; }
  f )
echo '=== the spelling is taken as written, never parsed'
( n=(a b c)
  f() { declare -n r='n[1+1]'; declare r=zz; declare -p 'n[1+1]' 2>&1; declare -p n; }
  f )
( n=(a b c)
  f() { declare -n r='n[ 1 ]'; declare r=zz; declare -p 'n[ 1 ]' 2>&1; }
  f )
( declare -A m=([k]=K)
  f() { declare -n r='m[k]'; declare r=zz; declare -p 'm[k]' 2>&1; declare -p m; }
  f )
( n=(a b c)
  f() { declare -n r='n[@]'; declare r=zz; echo "s=$?"; declare -p 'n[@]' 2>&1; declare -p n; }
  f )
echo '=== the attributes land on that name too'
( n=(a b c)
  f() { declare -n r='n[1]'; declare -i r=7+1; declare -p 'n[1]'; echo "r=[$r]"; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; declare -u r=abc; declare -p 'n[1]'; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; declare -x r=zz; declare -p 'n[1]'; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; declare -r r=zz; declare -p 'n[1]'; }
  f )
echo '=== a valueless operand declares it unvalued, attributes and all'
for w in 'declare r' 'local r' 'declare -i r' 'declare -x r' 'declare -l r' \
         'declare -r r' 'declare -a r' 'declare -A r'; do
  ( n=(a b c)
    f() { declare -n r='n[1]'; eval "$1"; echo "s=$?"
          declare -p 'n[1]' 2>&1; declare -p r; declare -p n; }
    f "$w" )
done
echo '=== the reference itself is untouched by any of it'
( n=(a b c)
  f() { declare -n r='n[1]'; declare r=zz; declare -p r; }
  f )
echo '=== any reference to that spelling finds it, from this frame or a deeper one'
( n=(a b c)
  g() { declare -n s='n[1]'; echo "s=[$s]"; s=QQ; echo "s2=[$s]"; }
  f() { declare -n r='n[1]'; declare r=zz; g; echo "r=[$r]"; declare -p n; }
  f )
( n=(a b c); declare -n s='n[1]'
  f() { declare -n r='n[1]'; declare r=zz; echo "in=[$s]"; }
  f; echo "out=[$s]" )
echo '=== …but a plain read of the element does not'
( n=(a b c)
  g() { echo "n1=[${n[1]}]"; }
  f() { declare -n r='n[1]'; declare r=zz; g; echo "direct=[${n[1]}]"; }
  f )
echo '=== a second declaration re-declares it; a plain assignment reaches it'
( n=(a b c)
  f() { declare -n r='n[1]'; declare r=zz; declare r=ww; echo "r=[$r]"; declare -p 'n[1]'; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; declare r=zz; r=ww; echo "r=[$r]"; declare -p 'n[1]'; declare -p n; }
  f )
echo '=== unset by that name takes the array element, not the local'
( n=(a b c)
  f() { declare -n r='n[1]'; declare r=zz; unset 'n[1]'; echo "r=[$r]"; declare -p n; }
  f )
echo '=== it is frame-scoped: a nested frame makes its own'
( n=(a b c)
  g() { declare -n r='n[1]'; declare r=ww; declare -p 'n[1]'; }
  f() { declare -n r='n[1]'; declare r=zz; g; echo "back r=[$r]"; declare -p 'n[1]'; }
  f; declare -p 'n[1]' 2>&1 )
echo '=== a reference to a nonexistent base is no different'
( f() { declare -n r='nosuch[1]'; declare r=zz; declare -p 'nosuch[1]' 2>&1; declare -p nosuch 2>&1; }
  f )
echo '=== only a local binding does it: top level stores the element'
( n=(a b c); declare -n r='n[1]'; declare r=zz; declare -p n; declare -p 'n[1]' 2>&1 )
( n=(a b c); declare -n r='n[1]'
  f() { declare -g r=zz; echo "s=$?"; }
  f; declare -p n; declare -p 'n[1]' 2>&1 )
echo '=== and a compound literal at top level is still refused'
( n=(a b c); declare -n r='n[1]'; declare r=(x y); echo "s=$?"; declare -p n )
echo '=== a plain reference is unaffected — the local is the target name'
( w=5
  f() { declare -n r=w; declare r=9; declare -p w; declare -p r; }
  f; declare -p w )
( f() { declare -n r=nosuch; declare r=(x y); declare -p nosuch; }
  f )
