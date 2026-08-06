# `declare -g` is not a switch. bash's option loop sets its `mkglobal` in the
# `-` direction only and never clears it, so a `+g` in the same command — before
# or after — does not bring the declaration back into the local scope it would
# otherwise have had.
#
# There is a second, undocumented letter for the same thing: `-G`, which forces
# global scope exactly as `-g` does and additionally asks bash's `chklocal` —
# "unless *this* frame already has a local of the name, in which case write
# that". A local belonging to an enclosing frame is stepped over just as `-g`
# steps over it, so the two letters differ only where the declaration and the
# local are in the same call. (This is the mode `export` and `readonly` carry
# standingly.) Neither letter is listed in the usage synopsis.
#
# A compound literal is not moved by `-G` at all: the assignment machinery looks
# for `A`, `a` or `g` on the command's words to decide where a literal binds,
# and `G` is none of them — so the literal makes a local and only the attribute
# pass that follows it goes global. (`-g` *is* one of them, so a literal under
# it binds globally as one would expect.)
#
# `local` is not exempt from any of this. bash's `local` is `declare` with one
# extra check in front of it — "am I in a function?" — and the very same option
# loop behind it, so `local -g y=1` writes the global and leaves any local of
# the name alone. Only the extra check is `local`'s own.

echo '=== +g does not take -g back'
( f() { declare -g +g w=1; }; f; declare -p w )
( f() { declare +g -g w=2; }; f; declare -p w )
( f() { declare +g w=3; }; f; declare -p w 2>&1 )
echo '=== -G is global where the frame has no local of the name'
( z=glob; f() { declare -G z=out; declare -p z; }; f; declare -p z )
( f() { declare -G fresh=made; }; f; declare -p fresh )
echo '=== …and writes this frame’s own local where it has one'
( y=glob; f() { local y=in; declare -G y=out; declare -p y; }; f; declare -p y )
( y=glob; f() { local y=in; declare -Gi y; declare -p y; }; f; declare -p y )
echo '=== an enclosing frame’s local is stepped over'
( y=glob; g() { declare -G y=out; }; f() { local y=in; g; declare -p y; }; f; declare -p y )
( y=glob; f() { local y=in; h() { declare -G y=out; }; h; }; f; declare -p y )
echo '=== -g does not prefer this frame’s local'
( y=glob; f() { local y=in; declare -g y=out; declare -p y; }; f; declare -p y )
echo '=== a compound literal under -G stays local'
( f() { declare -G n=(1 2); declare -p n; }; f; declare -p n 2>&1 )
( f() { declare -g n=(1 2); declare -p n; }; f; declare -p n 2>&1 )
echo '=== +G asks for nothing'
( declare +G x=1; declare -p x )
( y=glob; f() { local y=in; declare +G y=out; }; f; declare -p y )
echo '=== neither letter is refused, and neither is advertised'
( declare -G v=1; echo "s=$?" )
( typeset -G v=1; echo "s=$?" )
( declare -Z v=1; echo "s=$?" ) 2>&1
echo '=== -G with -p prints what it would have written'
( y=glob; f() { local y=in; declare -G -p y; }; f )
( y=glob; f() { declare -G -p y; }; f )
echo '=== and `local` takes both letters too'
( y=glob; f() { local -g y=in; declare -p y; }; f; declare -p y )
( y=glob; f() { local y=pre; local -g y=in; declare -p y; }; f; declare -p y )
( y=glob; f() { local -g y; declare -p y; }; f; declare -p y )
( f() { local -g n=(1 2); declare -p n; }; f; declare -p n 2>&1 )
( y=glob; f() { local -gi y=3+4; declare -p y; }; f; declare -p y )
( y=glob; f() { g() { local -g y=deep; }; local y=in; g; declare -p y; }; f; declare -p y )
( y=glob; f() { local -g +g y=in; }; f; declare -p y )
( y=glob; f() { local -G y=in; declare -p y; }; f; declare -p y )
( y=glob; f() { local y=pre; local -G y=in; declare -p y; }; f; declare -p y )
( local -g q=1 ); echo "s=$?"
