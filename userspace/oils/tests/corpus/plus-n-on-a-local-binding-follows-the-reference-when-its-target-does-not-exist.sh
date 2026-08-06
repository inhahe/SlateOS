# `declare +n` asks for the nameref attribute to come *off*, and the question is
# always which variable the command is about: the reference itself, or the thing
# it points at.
#
# Inside a function, where the declaration would bind a **local**, bash decides
# that by looking the target up: if a variable of that name already exists the
# command is about the reference — `+n` strips its attribute and the value it
# held stays behind as ordinary text. If nothing answers to that name, the
# declaration follows the reference after all and is about the target: the
# target is brought into being with whatever attributes and value were asked
# for, and the reference is left untouched, nameref attribute and all (the
# target never carried the attribute, so there is nothing there for `+n` to
# take off).
#
# "Exists" is bash's plain variable lookup, so a name brought into being
# *unvalued* by `declare nosuch` counts. A subscripted spelling never exists as
# a name on its own — so a reference to an element always follows — unless an
# earlier operand made one, which a local declaration through such a reference
# does (see
# a-local-declared-through-a-reference-to-an-element-is-named-by-the-spelling).
#
# A chain is walked to its end first: what matters is whether the *last* name
# exists, not the link in the middle.
#
# None of this applies where the declaration would not bind a local. At top
# level, and under `-g` inside a function, `+n` is always about the reference,
# and any other attribute in the same command follows the reference as it
# normally does.

echo '=== +n on a local binding: the target exists, so the operand keeps it'
( w=5
  f() { declare -n r=w; declare +n r; echo "s=$?"; declare -p w; declare -p r; }
  f; declare -p w )
( w=5
  f() { declare -n r=w; declare -x +n r; echo "s=$?"; declare -p w; declare -p r; }
  f )
( w=5
  f() { declare -n r=w; declare +n r=zz; echo "s=$?"; declare -p w; declare -p r; }
  f )
( f() { declare nosuch; declare -n r=nosuch; declare +n r; echo "s=$?"; declare -p nosuch; declare -p r; }
  f )
echo '=== …and when it does not, the declaration is about the target after all'
( f() { declare -n r=nosuch; declare +n r; echo "s=$?"; declare -p nosuch; declare -p r; }
  f )
( f() { declare -n r=nosuch; declare -x +n r; echo "s=$?"; declare -p nosuch; declare -p r; }
  f )
( f() { declare -n r=nosuch; declare -ir +n r; echo "s=$?"; declare -p nosuch; declare -p r; }
  f )
( f() { declare -n r=nosuch; declare +n r=zz; echo "s=$?"; declare -p nosuch; declare -p r; }
  f )
echo '=== the reference still reads and writes through afterwards'
( f() { declare -n r=nosuch; declare +n r=zz; echo "r=[$r]"; r=ww; declare -p nosuch; }
  f )
echo '=== the target it makes is local to the frame'
( f() { declare -n r=nosuch; declare +n r=zz; declare -p nosuch; }
  f; declare -p nosuch 2>&1 )
echo '=== an element spelling is never a name that exists, so it always follows'
( n=(a b c)
  f() { declare -n r='n[1]'; declare +n r; echo "s=$?"; declare -p 'n[1]'; declare -p r; declare -p n; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; declare -x +n r; echo "s=$?"; declare -p 'n[1]'; declare -p r; }
  f )
( n=(a b c)
  f() { declare -n r='n[1]'; declare +n r=zz; echo "s=$?"; declare -p 'n[1]'; declare -p r; declare -p n; }
  f )
echo '=== …until an earlier operand has made one, and then it does not'
( n=(a b c)
  f() { declare -n r='n[1]'; declare r=zz; declare +n r; echo "s=$?"; declare -p 'n[1]'; declare -p r; }
  f )
echo '=== at top level +n never follows'
( declare -n r=nosuch; declare +n r; echo "s=$?"; declare -p nosuch 2>&1; declare -p r )
( n=(a b c); declare -n r='n[1]'; declare +n r; echo "s=$?"; declare -p 'n[1]' 2>&1; declare -p r; declare -p n )
( n=(a b c); declare -n r='n[1]'; declare -x +n r; echo "s=$?"; declare -p 'n[1]' 2>&1; declare -p r; declare -p n )
echo '=== nor under -g inside a function'
( n=(a b c); declare -n r='n[1]'
  f() { declare -g +n r; echo "s=$?"; declare -p 'n[1]' 2>&1; declare -p r; }
  f )
( declare -n r=nosuch
  f() { declare -g +n r; echo "s=$?"; declare -p nosuch 2>&1; declare -p r; }
  f )
echo '=== a longer chain: the walk ends where it always did'
( w=5
  f() { declare -n m=w; declare -n r=m; declare +n r; echo "s=$?"; declare -p w m r; }
  f )
( f() { declare -n m=nosuch; declare -n r=m; declare +n r; echo "s=$?"; declare -p nosuch m r 2>&1; }
  f )
echo '=== +n on a name that is not a reference is a plain declaration'
( f() { declare q=1; declare +n q; echo "s=$?"; declare -p q; }
  f )
( f() { declare +n q=1; echo "s=$?"; declare -p q; }
  f )
