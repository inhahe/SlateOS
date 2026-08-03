# `command` and `builtin` both re-dispatch the words that follow them, but they
# are not grammar: each is an ordinary **regular builtin**, resolved exactly
# where every other one is. Three consequences follow from that, and all three
# are easy to get wrong by special-casing the two names too early:
#
#   * A **function** of the same name shadows the wrapper — in posix mode too,
#     since neither is *special*. `command() { …; }; command echo hi` runs the
#     function, and `builtin command echo hi` is how the real one is reached
#     past it.
#   * `enable -n` takes the wrapper away like any other builtin, after which the
#     shell goes looking for an **external** of that name and does not find one.
#   * Either can wrap the other, and itself, to any depth.
#
# What they do differ in is what they will re-dispatch *to*:
#
#   * `command` skips the function table, so a builtin or a `$PATH` file is what
#     it finds; a builtin turned off with `enable -n` sends it to the
#     same-named external.
#   * `builtin` searches only the builtin table — a keyword, a function or a
#     file is `not a shell builtin`, and so is a builtin `enable -n` turned off.
#   * `command` also strips the POSIX **special** class off whatever it runs, so
#     an assignment prefix on it never outlives the command even in posix mode.
#     `builtin` leaves that alone — but a usage error in a special builtin stops
#     being fatal under *either* of them.
#
# `command -v`/`-V` describe rather than run, and their `-V` miss is a
# diagnostic of the *command*, so it follows the command's own `2>` and not the
# shell's.

echo "=== a function shadows either wrapper"
command() { echo "  FUNC-command $*"; }
builtin() { echo "  FUNC-builtin $*"; }
command echo hi
builtin echo hi
echo "  but builtin reaches the real one:"
unset -f builtin
builtin command echo "  past the function"
unset -f command

echo "=== and posix mode does not change that (neither is special)"
( set -o posix; command() { echo "  FUNC2 $*"; }; command echo hi )

echo "=== enable -n takes the wrapper itself away"
( enable -n builtin; builtin echo hi ) 2>&1; echo "  rc=$?"
( enable -n command; command echo hi ) 2>&1; echo "  rc=$?"

echo "=== they nest, in either order and to any depth"
command command echo "  A"
builtin builtin echo "  B"
command builtin echo "  C"
builtin command echo "  D"
command builtin command builtin echo "  E"
x=$(command builtin echo F); echo "  captured=[$x]"
builtin command false; echo "  status passes through: $?"

echo "=== command skips functions; builtin skips everything but builtins"
echo() { printf '  OVERRIDE\n'; }
command echo "  bypassed"
builtin echo "  bypassed too"
unset -f echo
f() { :; }
builtin f 2>&1;  echo "  builtin f    rc=$?"
builtin if 2>&1; echo "  builtin if   rc=$?"
builtin ls 2>&1; echo "  builtin ls   rc=$?"

echo "=== a disabled builtin: command falls to the external, builtin refuses"
( enable -n echo; builtin echo hi ) 2>&1; echo "  rc=$?"
( enable -n true; command true ); echo "  command true rc=$?"

echo "=== command strips the special class, builtin does not"
( set -o posix; A=1 builtin eval ':' ; echo "  builtin eval: A=$A" )
( set -o posix; A=2 command eval ':' ; echo "  command eval: A=$A" )
( set -o posix; A=3 eval ':'         ; echo "  plain eval:   A=$A" )
# A usage error in a special builtin ends a posix-mode shell — but neither
# wrapper's re-dispatch counts as one, so both survive it. (This is the one
# thing `builtin` does share with `command`; the assignment prefix above is
# where the two part company.)
for pre in "" "command " "builtin "; do
  ( set -o posix; ${pre}unset -z x; echo "  reached" ) 2>/dev/null
  echo "  [${pre:-plain}] unset -z: rc=$?"
done

echo "=== the wrappers' own option words"
command -v command; command -v builtin
builtin -- echo "  builtin --"
command -- echo "  command --"
command -pz f 2>&1; echo "  rc=$?"
builtin -z 2>&1;    echo "  rc=$?"
command; echo "  command alone rc=$?"
builtin; echo "  builtin alone rc=$?"

echo "=== command -V describes; a miss follows the command's own stderr"
command -V while
command -V cd
f2() { :; }
command -V f2
y=$(command -V no_such_xyz 2>&1); echo "  captured=[$y] rc=$?"
command -V no_such_xyz 2>/dev/null; echo "  silenced      rc=$?"
command -V no_such_xyz 2>&-;        echo "  closed        rc=$?"
z=$(command -V a_xyz b_xyz 2>&1); echo "  both=[$z]"
w=$(command -v no_such_xyz 2>&1); echo "  -v is quiet=[$w] rc=$?"
echo "=== an alias is described only where it would expand"
alias al='echo A'
command -V al 2>&1; echo "  rc=$?"
shopt -s expand_aliases
command -V al; command -v al
