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
# Writing through the name from somewhere else — `read`, a `for` loop's control
# variable — fails too, and there the status is the only sign of it.
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
( COMP_WORDBREAKS=xy; echo "[$COMP_WORDBREAKS] rc=$?" )
