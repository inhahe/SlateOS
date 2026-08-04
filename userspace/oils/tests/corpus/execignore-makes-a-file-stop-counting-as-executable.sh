# `$EXECIGNORE` is a `:`-separated list of patterns naming files a command
# *lookup* should pretend it cannot execute — meant for shared libraries that
# carry the exec bit without being programs.
#
# Three details decide what it actually does:
#
#   * each pattern is matched against the **whole** candidate path, not per
#     component, so `*` crosses a `/`: `bin/tool` is hidden by `*tool`, by
#     `bin/*` and by `bin/tool`, but not by `tool` and not by `*/bin/*`;
#   * the match **folds case** — always, whatever `nocasematch`/`nocaseglob`
#     say, because bash passes `FNM_CASEFOLD` here and nowhere near `[[ == ]]`.
#     So `EXECIGNORE=bin/TOOL` hides `bin/tool` while `[[ bin/tool == bin/TOOL ]]`
#     is still false;
#   * `shopt extglob` is honoured, so `bin/@(a|b)` is a list only when it is on.
#
# And what it does *not* reach is as much of the answer: it filters a `$PATH`
# search and what `type`/`command -v` say about a word that already spelled a
# path — but not *running* that word, not `.`/`source` (which looks for a
# readable file, not an executable one), and not a `hash` hit (which never
# reaches the search at all).
#
# Everything else about the name is ordinary: it has no array meaning, so a list
# assignment leaves element 0 doing the work; it obeys `local`; an unparseable
# pattern is simply a pattern that matches nothing.

mkdir -p bin
for n in tool tool.so helper libfoo.so; do
  printf '#!/bin/sh\necho %s\n' "$n" > bin/$n
done
printf 'echo "  sourced"\n' > bin/lib.sh
printf 'echo "  plain ran"\n' > bin/plain
chmod +x bin/tool bin/tool.so bin/helper bin/libfoo.so bin/lib.sh bin/plain
PATH=bin

echo "=== with nothing ignored"
type -a tool.so
command -v libfoo.so

echo "=== the pattern matches the whole path, and * crosses a slash"
for p in '*.so' '*/*.so' 'bin/*' '*/bin/*' 'tool' '*/tool' 'bin/tool' '*tool'; do
  EXECIGNORE=$p
  printf '  [%s] cv=[%s] st=%s\n' "$p" "$(command -v tool 2>/dev/null)" "$?"
done
unset EXECIGNORE

echo "=== the list is colon-separated, and empty entries are no entries"
EXECIGNORE='*/nope:bin/helper'
echo "  helper=[$(command -v helper)] tool=[$(command -v tool)]"
EXECIGNORE='::bin/helper:'
echo "  helper=[$(command -v helper)]"
EXECIGNORE=
echo "  empty helper=[$(command -v helper)]"
unset EXECIGNORE

echo "=== type -a and type -P ask the same question"
EXECIGNORE='bin/tool.so'
type -a tool.so; echo "  ta st=$?"
type -P tool.so; echo "  tP st=$?"
type -a tool; echo "  ok st=$?"
unset EXECIGNORE

echo "=== a word that spelled a path is described but still runs"
EXECIGNORE='bin/helper:bin/plain'
command -v bin/helper; echo "  st=$?"
command -v ./bin/helper; echo "  st=$?"
bin/plain; echo "  run st=$?"
unset EXECIGNORE

echo "=== the match folds case"
EXECIGNORE='bin/TOOL'; echo "  a=[$(command -v tool)]"
EXECIGNORE='bin/tOOl'; echo "  b=[$(command -v tool)]"
EXECIGNORE='BIN/tool'; echo "  c=[$(command -v tool)]"
EXECIGNORE='xin/tool'; echo "  d=[$(command -v tool)]"
echo "  and [[ ]] does not: $([[ bin/tool == bin/TOOL ]] && echo y || echo n)"
unset EXECIGNORE

echo "=== extglob is honoured"
EXECIGNORE='bin/@(tool|helper)'
echo "  off=[$(command -v tool)]"
shopt -s extglob
echo "  on=[$(command -v tool)]"
shopt -u extglob
unset EXECIGNORE

echo "=== a pattern that will not parse simply matches nothing"
EXECIGNORE='['
echo "  a=[$(command -v tool)] st=$?"
unset EXECIGNORE

echo "=== source looks for a readable file, not an executable one"
EXECIGNORE='bin/lib.sh'
. lib.sh; echo "  st=$?"
unset EXECIGNORE

echo "=== it obeys local, and is an ordinary scalar otherwise"
EXECIGNORE='bin/helper'
f() { local EXECIGNORE='bin/tool'; echo "  in=[$(command -v tool)][$(command -v helper)]"; }
f
echo "  out=[$(command -v tool)][$(command -v helper)]"
declare -p EXECIGNORE
unset EXECIGNORE

echo "=== a list assignment leaves element 0 doing the work"
EXECIGNORE=(bin/tool bin/helper)
echo "  arr=[$(command -v tool)][$(command -v helper)]"
unset EXECIGNORE

echo "=== a pattern may name the directory as easily as the file"
EXECIGNORE='bin*'
echo "  d=[$(command -v tool)]"
unset EXECIGNORE
