# Six variables refuse to be assigned to.
#
# `FUNCNAME`, `GROUPS`, `BASH_SOURCE`, `BASH_LINENO`, `BASH_ARGC` and
# `BASH_ARGV` are maintained by the shell, and bash marks them so that a
# script's attempt to set one is not obeyed. The refusal is *silent* — there is
# no diagnostic anywhere below — and it takes two shapes that differ only in
# what they leave behind in `$?`:
#
#   * a scalar assignment (`GROUPS=5`, `FUNCNAME[0]=x`, `GROUPS+=x`) is simply
#     not performed, and the command succeeds anyway;
#   * an array literal (`GROUPS=(1 2)`) is not performed and *fails*, which
#     abandons the rest of the parse unit exactly as assigning to a readonly
#     variable does — so a `;`-separated command after it never runs, while one
#     on the next line does.
#
# Writing through the name from somewhere else — `read`, `printf -v`, `mapfile`,
# a `for` loop's control variable — fails too, and there the status is the only
# sign of it. An *arithmetic* write behaves like an arithmetic error: it abandons
# the expression where it stands, so `(( ))`/`let` report 1 while `$(( ))` is
# fatal to the command list — and it says nothing either.
#
# `unset` drops the attribute along with every other one, after which the name
# is ordinary. `DIRSTACK` and `COMP_WORDBREAKS` look like they belong to the
# family and do not.
#
# `$GROUPS` holds the invoking user's group IDs, which differ between the two
# shells by construction, so nothing here prints its value.

echo "=== a scalar assignment is ignored, and succeeds anyway"
GROUPS=5; echo "rc=$?"
GROUPS[0]=5; echo "rc=$?"
GROUPS+=x; echo "rc=$?"
FUNCNAME=x; echo "rc=$? [${FUNCNAME[0]-unset}]"
BASH_LINENO=x; echo "rc=$? [${BASH_LINENO[0]-unset}]"
# BASH_ARGC/BASH_ARGV hold nothing here either, but only osh says so — see
# known-issues TD-OILS-MISSING-SPECIAL-ARRAYS for bash's undocumented
# non-extdebug base frame — so this checks the status alone.
BASH_ARGC=x; echo "rc=$?"
BASH_ARGV=x; echo "rc=$?"

echo "=== an array literal fails, and abandons the rest of the parse unit"
( FUNCNAME=(1 2); echo unreachable ); echo "rc=$?"
( FUNCNAME+=(1); echo unreachable ); echo "rc=$?"
( BASH_SOURCE=(1); echo unreachable ); echo "rc=$?"
(
FUNCNAME=(1 2)
echo "next line still runs, rc=$?"
); echo "rc=$?"

echo "=== a write through the name fails, and the status is the only sign"
read FUNCNAME <<<hi; echo "rc=$?"
for FUNCNAME in a b; do echo body; done; echo "rc=$?"
for FUNCNAME in; do echo body; done; echo "rc=$?"
printf -v GROUPS x; echo "rc=$?"
printf -v 'GROUPS[0]' x; echo "rc=$?"
mapfile GROUPS <<<hi; echo "rc=$?"
mapfile -t BASH_SOURCE <<<hi; echo "rc=$?"
read -a GROUPS <<<hi; echo "rc=$?"
# None of them abandons the parse unit the way an array literal does.
printf -v GROUPS x; echo "and the next command still runs"

echo "=== arithmetic refuses it as if the expression had errored, but silently"
(( GROUPS = 5 )); echo "rc=$?"
(( GROUPS[0] = 5 )); echo "rc=$?"
(( GROUPS++ )); echo "rc=$?"
let GROUPS=7; echo "rc=$?"
# The refusal stops the expression where it stands, so an assignment before it
# in a comma list stands and one after it never happens — as a division by zero
# would, only without the diagnostic.
x=9; (( x = 3, GROUPS = 5 )); echo "rc=$? x=$x"
x=9; (( GROUPS = 5, x = 3 )); echo "rc=$? x=$x"
# And in *expansion* position it is fatal to the command list, again like any
# other arithmetic error.
( echo "[$(( GROUPS = 5 ))]"; echo unreachable ); echo "rc=$?"

echo "=== a declaration builtin refuses it by where the binding would land"
# At global scope the value is dropped and the attributes are the only question:
# `declare` applies none of them and reports 1, while `export` and `readonly`
# apply theirs and report 0. A valueless `declare` asks for no assignment, so
# nothing is refused and `-u` goes on as usual.
( declare -u GROUPS=5; echo "rc=$?"; declare -p GROUPS ) | sed 's/=(.*)/=(...)/'
( declare -x GROUPS=5; echo "rc=$?"; declare -p GROUPS ) | sed 's/=(.*)/=(...)/'
( declare -u GROUPS; echo "rc=$?"; declare -p GROUPS ) | sed 's/=(.*)/=(...)/'
( declare GROUPS[0]=5; echo "rc=$?" )
( export GROUPS=5; echo "rc=$?"; declare -p GROUPS ) | sed 's/=(.*)/=(...)/'
( readonly GROUPS=5; echo "rc=$?"; declare -p GROUPS ) | sed 's/=(.*)/=(...)/'
# A later operand of the same command still binds.
( declare GROUPS=5 z=9; echo "rc=$?"; declare -p z )
# A *local* one is the one shape that says anything. Even the valueless form,
# because making a local of the name is itself the refused assignment.
( f() { local GROUPS=5; echo "rc=$?"; }; f ) 2>&1
( f() { local GROUPS; echo "rc=$?"; }; f ) 2>&1
( f() { declare GROUPS; echo "rc=$?"; }; f ) 2>&1
( f() { local GROUPS[0]=5; echo "rc=$?"; }; f ) 2>&1
( f() { local GROUPS x=1; echo "rc=$?"; declare -p x; }; f ) 2>&1
# `-g` and `export` name the global, so they are refused the global way.
( f() { declare -g GROUPS; echo "rc=$?"; }; f ) 2>&1
( f() { declare -g GROUPS=5; echo "rc=$?"; }; f ) 2>&1
( f() { export GROUPS=5; echo "rc=$?"; }; f ) 2>&1

echo "=== and a compound literal splits the same way, twice over"
# The local refusal is reported by the compound-assignment machinery — which
# inside a function tags its diagnostics with the function's name — and then
# again by the builtin it is handed to.
( zebra() { declare GROUPS=(1 2); echo "rc=$?"; }; zebra ) 2>&1
( zebra() { local -a GROUPS=(1 2) ok=(3); echo "rc=$?"; declare -p ok; }; zebra ) 2>&1
( zebra() { declare GROUPS=(1 2) after=(9); declare -p after; }; zebra ) 2>&1
( zebra() { declare -g GROUPS=(1 2); echo "rc=$?"; }; zebra ) 2>&1
( zebra() { export GROUPS=(1 2); echo "rc=$?"; }; zebra ) 2>&1
( f() { declare -gux GROUPS=(1 2); }; f; declare -p GROUPS ) 2>&1 | sed 's/=(.*)/=(...)/'
( f() { declare -ux GROUPS=(1 2); }; f; declare -p GROUPS ) 2>&1 | sed 's/=(.*)/=(...)/'
# At top level a compound is not special-cased at all: it takes the same silent
# parse-unit discard a bare `GROUPS=(1 2)` does, so the `after` never binds.
( declare GROUPS=(1 2) after=(9); declare -p after ); echo "rc=$?"
# `export` and `readonly` bind at the global scope wherever they are invoked, so
# their literal survives the call — where `declare -r`'s does not.
( f() { export q=(1 2); }; f; declare -p q ) 2>&1
( f() { readonly q=(1 2); }; f; declare -p q ) 2>&1
( f() { declare -r q=(1 2); }; f; declare -p q ) 2>&1

echo "=== the shell's own value is untouched by any of it"
f() { echo "[${FUNCNAME[0]}]"; }
FUNCNAME=z; f
FUNCNAME[0]=z; f

echo "=== unset drops the attribute; the name is then ordinary"
( unset FUNCNAME; FUNCNAME=(1 2); echo "[${FUNCNAME[*]}] rc=$?" )
( unset GROUPS; GROUPS=9; echo "[$GROUPS] rc=$?" )

echo "=== but four of the six cannot be unset at all"
( unset BASH_SOURCE; echo "rc=$?" )
( unset BASH_LINENO; echo "rc=$?" )
( unset BASH_ARGC; echo "rc=$?" )
( unset BASH_ARGV; echo "rc=$?" )
# `unset -v` names the variable explicitly; `unset -f` names a function, and
# there is none, so it is silently fine.
( unset -v BASH_SOURCE; echo "rc=$?" )
( unset -f BASH_SOURCE; echo "rc=$?" )

echo "=== set -x traces the assignment it is about to ignore"
( set -x; GROUPS=5; GROUPS[0]=6; FUNCNAME=x ) 2>&1
( set -x; FUNCNAME=(1 2) ) 2>&1

echo "=== DIRSTACK and COMP_WORDBREAKS are not in the family"
# `COMP_WORDBREAKS` is an ordinary variable. `DIRSTACK` is dynamic in the
# *other* direction — the write is pushed back into the directory stack rather
# than refused — which dirstack-var.sh covers on its own.
( COMP_WORDBREAKS=xy; echo "[$COMP_WORDBREAKS] rc=$?" )
