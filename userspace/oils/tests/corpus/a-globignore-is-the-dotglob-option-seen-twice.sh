# `GLOBIGNORE` and the `dotglob` option are one setting, not two.
#
# The manual says a non-null `GLOBIGNORE` "has the effect of enabling the
# dotglob shell option", and that is literal: assigning the name turns the
# option itself on, so `shopt dotglob` reports it. The consequences are the ones
# a shared setting has and a derived rule would not —
#
#   * losing the variable turns the option off, even if the user set it
#     themselves: `shopt -s dotglob; GLOBIGNORE=x; unset GLOBIGNORE` ends with
#     dotglob off;
#   * `shopt -u dotglob` while `GLOBIGNORE` is set really does stop leading-dot
#     names matching, and stays won until the next assignment;
#   * a value that is set but empty moves nothing.
#
# The filtering is the variable's own job and is unaffected by any of that.

p() { printf '  [%s]' "$@"; echo; }
mkdir -p g && cd g || exit 1
: > a.txt; : > b.txt; : > c.log; : > .hidden; : > .other; mkdir -p sub

echo "=== the option follows the variable"
shopt dotglob
GLOBIGNORE='*.log'; shopt dotglob
p *
unset GLOBIGNORE; shopt dotglob
p *

echo "=== and it takes the user's own setting with it"
shopt -s dotglob; shopt dotglob
GLOBIGNORE=x; shopt dotglob
unset GLOBIGNORE; shopt dotglob
p *

echo "=== set but empty moves nothing"
shopt -s dotglob
GLOBIGNORE=; shopt dotglob
unset GLOBIGNORE; shopt dotglob

echo "=== the option can be turned off underneath a set variable"
GLOBIGNORE='*.log'; shopt dotglob
p *
shopt -u dotglob; shopt dotglob
p *
GLOBIGNORE='*.log'; shopt dotglob
p *
unset GLOBIGNORE

echo "=== the filtering is the variable's own job"
GLOBIGNORE='*.txt'
p *
p *.txt
p .*
GLOBIGNORE='*.log:b.txt'
p *
GLOBIGNORE='*'
p *
unset GLOBIGNORE

echo "=== the patterns are globs, matched against the whole generated name"
GLOBIGNORE='?.txt'
p *
GLOBIGNORE='[ab].txt'
p *
GLOBIGNORE='sub'
p *
p sub/../*
unset GLOBIGNORE

echo "=== it filters before nullglob and failglob see the result"
GLOBIGNORE='*.txt:*.log:sub:.*'
shopt -s nullglob
p *
echo "  n=$#"
shopt -u nullglob
p *
unset GLOBIGNORE
shopt -u dotglob

echo "=== a scope that ends is the name losing its value"
f() { local GLOBIGNORE=z; shopt dotglob; }
f; shopt dotglob
GLOBIGNORE=q shopt dotglob
shopt dotglob

echo "=== and the option shows up where options are listed"
GLOBIGNORE=x
case $BASHOPTS in *dotglob*) echo "  in BASHOPTS";; *) echo "  not in BASHOPTS";; esac
shopt -p dotglob
shopt -q dotglob; echo "  -q st=$?"
unset GLOBIGNORE
case $BASHOPTS in *dotglob*) echo "  in BASHOPTS";; *) echo "  not in BASHOPTS";; esac
shopt -p dotglob
shopt -q dotglob; echo "  -q st=$?"
