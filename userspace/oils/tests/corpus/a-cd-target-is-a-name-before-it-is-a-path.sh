# `cd` is handed a *name*, and the name is not always a path. Three of them —
# an empty operand, an empty `$HOME` with no operand, and an empty `$OLDPWD`
# via `cd -` — name nothing at all, and in the default logical mode an empty
# name resolves against the current directory and arrives back where it
# started. So the cwd and `$PWD` are untouched, but the move counts: `$OLDPWD`
# becomes the directory that was not left, and `cd -` still announces it — an
# empty line. `cd -P` is the exception: there the empty name goes to the OS
# unresolved and fails with ENOENT, leaving `$OLDPWD` alone. An *unset* `$HOME`
# is a different thing again — not an empty directory name but no name at all,
# refused before anything happens.
#
# What `cd` prints on success is one of two different strings. `cd -` echoes
# `$OLDPWD` exactly as it was stored, unresolved: `../proj/.` prints as
# `../proj/.` even under `-P`. A `CDPATH` match echoes where it arrived, which
# is the point of the message — the name typed says nothing about which entry
# answered it — except under `-P`, where bash prints the joined name instead.
# An *empty* `CDPATH` entry means "here" and so announces nothing; `.` is not
# empty, and does.
#
# The `CDPATH` search itself is reached only from the branch that took the
# directory from the command line. Neither `cd` (which uses `$HOME`) nor `cd -`
# (which uses `$OLDPWD`) consults it, however relative the name found there
# turns out to be — so a relative `$HOME` simply fails. An entry that already
# ends in `/` is joined without a second one, and an empty operand is searched
# for like any other, landing in the entry itself.
#
# Deliberately absent:
#
#   * every absolute path. The two shells spell the same host directory
#     differently (`/d/x` vs `D:/x`), so nothing here prints `$PWD`, `$OLDPWD`
#     or a `CDPATH` announcement in full — only the last component. The one
#     exception is the `-P` announcement, which is the *joined* name and so is
#     as relative as the entry that made it.
#   * `cd -e` and `cd -@`. Both parse, and `-e` has a status to report when
#     `-P` cannot name the directory it reached, which does not arise here.
#   * `cd` with no operand and no `$HOME` *and* a `CDPATH` that would answer —
#     covered by the relative-`$HOME` row, which is the same branch.
#
# Every probe runs in a subshell so a directory change cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
mkdir -p top/here/deep top/proj
: > top/here/afile
cd top/here || exit 1
exec 4>&2 2>err

echo "=== an empty target is the directory you are already in"
( cd ''; echo "  operand rc=$? pwd=${PWD##*/} old=${OLDPWD##*/}" )
( HOME=; cd; echo "  home    rc=$? pwd=${PWD##*/} old=${OLDPWD##*/}" )
( OLDPWD=; cd -; echo "  dash    rc=$? pwd=${PWD##*/} old=${OLDPWD##*/}" )
( cd -- ''; echo "  --      rc=$? pwd=${PWD##*/} old=${OLDPWD##*/}" )
( cd ..; cd ''; echo "  moved   rc=$? pwd=${PWD##*/} old=${OLDPWD##*/}" )

echo "=== but under -P the empty name reaches the OS unresolved"
( OLDPWD=/keepme; cd -P ''; echo "  operand rc=$? pwd=${PWD##*/} old=$OLDPWD" )
( OLDPWD=/keepme; HOME=; cd -P; echo "  home    rc=$? pwd=${PWD##*/} old=$OLDPWD" )

echo "=== an unset HOME is not a directory at all"
( unset HOME; cd; echo "  plain rc=$? pwd=${PWD##*/}" )
( unset HOME; cd -L; echo "  -L    rc=$? pwd=${PWD##*/}" )
( unset HOME; cd deep; echo "  arg   rc=$? pwd=${PWD##*/}" )
( unset OLDPWD; cd -; echo "  old   rc=$? pwd=${PWD##*/}" )

echo "=== cd - echoes the name it was given, not the path it reached"
( OLDPWD=../proj/.; cd -; echo "  dot   rc=$? pwd=${PWD##*/}" )
( OLDPWD=../proj/; cd -; echo "  slash rc=$? pwd=${PWD##*/}" )
( OLDPWD=; cd -; echo "  empty rc=$? pwd=${PWD##*/}" )
( OLDPWD=../proj/.; cd -P -; echo "  phys  rc=$? pwd=${PWD##*/}" )

echo "=== CDPATH answers an operand, and only an operand"
( CDPATH=..; cd proj >/dev/null; echo "  operand rc=$? pwd=${PWD##*/}" )
( CDPATH=..; HOME=proj; cd; echo "  home    rc=$? pwd=${PWD##*/}" )
( CDPATH=..; OLDPWD=proj; cd -; echo "  dash    rc=$? pwd=${PWD##*/}" )
( CDPATH=..; cd ./proj; echo "  dotted  rc=$? pwd=${PWD##*/}" )
( CDPATH=..; cd '' >/dev/null; echo "  empty   rc=$? pwd=${PWD##*/}" )

echo "=== and what it announces depends on the entry that answered"
( CDPATH=..;  x=$(cd proj);    echo "  named rc=$? tail=[${x##*/}]" )
( CDPATH=.;   x=$(cd deep);    echo "  dot   rc=$? tail=[${x##*/}]" )
( CDPATH=:..; x=$(cd deep);    echo "  empty rc=$? tail=[${x##*/}]" )
( CDPATH=..;  x=$(cd -P proj); echo "  phys  rc=$? out=[$x]" )
( CDPATH=../; x=$(cd proj);    echo "  slash rc=$? tail=[${x##*/}]" )

echo "=== the ordinary refusals"
( cd nosuch;      echo "  bad   rc=$? pwd=${PWD##*/}" )
( cd afile;       echo "  file  rc=$? pwd=${PWD##*/}" )
( cd deep proj;   echo "  two   rc=$?" )
( cd -q;          echo "  opt   rc=$?" )
( cd deep; cd ..; echo "  round rc=$? pwd=${PWD##*/} old=${OLDPWD##*/}" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
