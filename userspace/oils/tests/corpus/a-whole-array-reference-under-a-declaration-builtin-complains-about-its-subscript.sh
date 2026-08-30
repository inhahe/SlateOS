# A nameref whose value spells a *whole array* — `n[@]`, `n[*]` — designates no
# single element, and a declaration builtin that follows one into the subscript
# says so: `n[@]: bad array subscript`, untagged, from the read rather than from
# the builtin. A compound literal earns it twice over, the refusal to treat the
# spelling as a name following on its heels; a valueless operand earns it alone,
# declares nothing, and succeeds.
#
# What decides whether the line appears is whether the command asks anything of
# the variable. A declaration with something to do reaches for the reference's
# **base** and does it there, and never passes the subscript at all; one with
# nothing to do just binds the operand, and that is the read that complains.
#
# The two operand shapes draw the line in different places, because they take
# different routes. The *scalar* operand goes to the builtin as written, so
# every letter that names an attribute counts, in either direction — `-r`, `+r`,
# `-t`, `-n`, `+n` and `-g` all silence it. The *compound* one is rebuilt by the
# assignment machinery first, which keeps only the array kind (`-a`, `-A`), the
# scope (`-g`, and the standing global scope of `export` and `readonly`) and the
# letters that transform a value as it binds (`i`, `l`, `u`, `c`) — so there
# `-r`, `-t` and `-p` leave the line standing and only the kept letters take it
# away. `-I` asks nothing of a variable on either route and never silences it.
#
# Inside a function neither route reaches for anything: the declaration binds a
# local named by the whole spelling, which is a name and not a reference, and
# nothing is complained about.

echo '=== a compound literal is complained about twice'
( n=(a b c); declare -n r='n[@]'; declare r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[*]'; declare r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; typeset r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; local r=(x y); echo "s=$?" )
echo '=== an element reference is complained about once'
( n=(a b c); declare -n r='n[1]'; declare r=(x y); echo "s=$?" )
echo '=== the base makes no difference'
( declare -A m=([k]=K); declare -n r='m[@]'; declare r=(x y); echo "s=$?" )
( declare -n r='nosuch[@]'; declare r=(x y); echo "s=$?" )
( n=hello; declare -n r='n[@]'; declare r=(x y); echo "s=$?" )
echo '=== the kind, the scope and the value letters take the line away'
( n=(a b c); declare -n r='n[@]'; declare -a r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -A r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -g r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -i r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -l r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare +u r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare +c r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; export r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; readonly r=(x y); echo "s=$?" )
echo '=== every other letter leaves it standing'
( n=(a b c); declare -n r='n[@]'; declare -r r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -x r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -t r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -n r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -p r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -I r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare +a r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare +g r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -- r=(x y); echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -rI r=(x y); echo "s=$?" )
echo '=== a valueless operand is complained about alone, and succeeds'
( n=(a b c); declare -n r='n[@]'; declare r; echo "s=$?"; declare -p r; declare -p n )
( n=(a b c); declare -n r='n[*]'; declare r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; typeset r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -- r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -I r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare +g r; echo "s=$?" )
( n=(a b c); declare -n q='n[@]'; declare -n r=q; declare r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare r x=1; echo "s=$?"; declare -p x )
( declare -n r='nosuch[@]'; declare r; echo "s=$?"; declare -p nosuch 2>&1 )
echo '=== and there any attribute at all takes it away'
( n=(a b c); declare -n r='n[@]'; declare -a r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -i r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -r r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -t r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -x r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare +r r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -g r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -n r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare +n r; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -p r; echo "s=$?" )
echo '=== an element reference asks nothing either'
( n=(a b c); declare -n r='n[1]'; declare r; echo "s=$?" )
echo '=== a valued operand is complained about whatever the command asks'
( n=(a b c); declare -n r='n[@]'; declare r=v; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -i r=v; echo "s=$?" )
( n=(a b c); declare -n r='n[@]'; declare -t r=v; echo "s=$?" )
echo '=== an ordinary assignment gives one line'
( n=(a b c); declare -n r='n[@]'; r=(x y); echo "s=$?"; declare -p n )
echo '=== and inside a function nothing is reached for at all'
( n=(a b c)
  f() { declare -n r='n[@]'; declare r=(x y); echo "s=$?"; declare -p 'n[@]'; declare -p n; }
  f )
( n=(a b c)
  f() { declare -n r='n[@]'; declare r; echo "s=$?"; declare -p 'n[@]' 2>&1; }
  f )
( n=(a b c)
  f() { declare -n r='n[@]'; declare -g r=(x y); echo "s=$?"; declare -p n; }
  f )
( n=(a b c)
  f() { declare -n r='n[@]'; declare -g r; echo "s=$?"; declare -p n; }
  f )
( n=(a b c)
  f() { declare -n r='n[@]'; local r; echo "s=$?"; declare -p 'n[@]' 2>&1; }
  f )
