# `getopts` remembers where it got to in *two* places, and only one of them is
# `OPTIND`. The variable says which argument a scan resumes at; a second,
# private cursor says how far into that argument's letters the scan already is.
# bash keeps them apart — `sh_optind` and `sh_charindex`, with `sh_curopt`
# recording which argument the letter cursor belongs to — and the pair is what
# makes a bundle like `-abc` yield three options while `OPTIND` sits still.
#
# Because the letter cursor is pinned to an argument *index* rather than to a
# vector, it survives things that ought to invalidate it. Handing the next call
# a different argument list resumes at the same offset into the new list's first
# argument, so a half-read `-abc` continues at letter two of `-xyz`. Replacing
# the positionals with `set --` does the same.
#
# And the two cursors are moved by different hands. Writing `OPTIND` runs a hook
# that moves the argument cursor, but the letter cursor is only thrown away when
# the value written is one bash calls *start over* — 1, 0, or anything negative.
# Any other number moves the argument cursor and leaves the letters where they
# were, which is how `OPTIND=3` mid-bundle can answer with a letter from an
# argument that is not the third.

echo "=== a bundle is read letter by letter with OPTIND standing still"
OPTIND=1; set -- -abc x
while getopts "abc" o; do echo "  o=$o ind=$OPTIND"; done
echo "  end rc=$? ind=$OPTIND"

echo "=== the letter cursor follows the argument index into a new list"
OPTIND=1; set -- -abc
getopts "abc" o; echo "  first o=$o ind=$OPTIND"
getopts "abcxyz" o -xyz -q; echo "  next rc=$? o=$o ind=$OPTIND"

echo "=== and the option letter it lands on need not be in the optstring"
OPTIND=1; set -- -abc
getopts "abc" o; echo "  first o=$o ind=$OPTIND"
getopts "abc" o -xyz -q; echo "  next rc=$? o=$o arg=[${OPTARG-unset}] ind=$OPTIND"

echo "=== replacing the positionals resumes at the same offset"
OPTIND=1; set -- -abcd
getopts "abcd" o; echo "  first o=$o ind=$OPTIND"
set -- -wxyz
getopts "wxyz" o; echo "  next rc=$? o=$o ind=$OPTIND"

echo "=== a shorter list can leave the cursor past the end of the argument"
OPTIND=1; set -- -ab
getopts "ab" o; echo "  first o=$o ind=$OPTIND"
set -- -a
getopts "ab" o; echo "  next rc=$? o=$o ind=$OPTIND"

echo "=== a function's own argument list is another list like any other"
f() { getopts "abcd" o "$@"; echo "  in rc=$? o=$o ind=$OPTIND"; }
OPTIND=1; set -- -abcd
getopts "abcd" o; echo "  first o=$o ind=$OPTIND"
f -wxyz

echo "=== 1, 0 and negative throw the letters away; other numbers do not"
OPTIND=1; set -- -ab -cd
getopts "abcd" o; echo "  first o=$o ind=$OPTIND"
OPTIND=1; getopts "abcd" o; echo "  after OPTIND=1 o=$o ind=$OPTIND"

OPTIND=1; set -- -ab -cd
getopts "abcd" o; echo "  first o=$o ind=$OPTIND"
OPTIND=0; getopts "abcd" o; echo "  after OPTIND=0 o=$o ind=$OPTIND"

OPTIND=1; set -- -ab -cd
getopts "abcd" o; echo "  first o=$o ind=$OPTIND"
OPTIND=-3; getopts "abcd" o; echo "  after OPTIND=-3 o=$o ind=$OPTIND"

OPTIND=1; set -- -ab -cd
getopts "abcd" o; echo "  first o=$o ind=$OPTIND"
OPTIND=3; getopts "abcd" o; echo "  after OPTIND=3 o=$o ind=$OPTIND"

OPTIND=1; set -- -abcd
getopts "abcd" o; echo "  first o=$o ind=$OPTIND"
OPTIND=2; getopts "abcd" o; echo "  after OPTIND=2 o=$o ind=$OPTIND"

echo "=== unsetting OPTIND is the empty value, which is start over"
OPTIND=1; set -- -ab
getopts "ab" o; echo "  first o=$o ind=$OPTIND"
unset OPTIND; getopts "ab" o; echo "  after unset o=$o ind=$OPTIND"

OPTIND=1; set -- -ab
getopts "ab" o; echo "  first o=$o ind=$OPTIND"
OPTIND=; getopts "ab" o; echo "  after empty o=$o ind=$OPTIND"

echo "=== the builtin's own write of OPTIND does not run the hook"
OPTIND=1; set -- -ab
getopts "ab" o; echo "  first o=$o ind=$OPTIND"
getopts "ab" o; echo "  second o=$o ind=$OPTIND"
getopts "ab" o; echo "  end rc=$? ind=$OPTIND"
