# `DIRSTACK` is a *view* of the directory stack, in both directions.
#
# bash gives the name a value function and an assign function, so it is not an
# array that `pushd` happens to keep updated:
#
#   * every read re-materialises it from the live stack, with the current
#     directory as element 0 and the saved entries after it;
#   * every element written is pushed back *into* the stack, so `dirs +1` and a
#     later `popd` see the new value.
#
# The assign function is called once per element, and it owns only the saved
# entries — which is where all of the odd-looking behaviour below comes from.
# Element 0 is the current directory, which the stack does not own, so writing
# it is ignored and never moves the shell; an element past the end has nowhere
# to go and is dropped. Since it is element*wise*, a whole-array assignment does
# not replace the array either: `DIRSTACK=(a b c)` against a two-deep stack
# ignores `a`, writes `b` and throws `c` away, leaving the array two long. Every
# one of these succeeds, and none of them says anything.
#
# `unset DIRSTACK` drops the hooks along with the variable and bash never puts
# them back: the name is ordinary from then on, `pushd` stops touching it, and a
# later `DIRSTACK=(a b c)` really is a three-element array. A `local DIRSTACK`
# is ordinary too, for the length of the call — the hooks belong to the global.
#
# Every path is absolute and host-specific, so nothing here prints one: the
# checks are on lengths, on statuses, and on comparisons against `$PWD`.

mkdir -p sub1 sub2
cd sub1
ROOT=$PWD

echo "=== a member assignment writes through to the stack"
( pushd "$ROOT/../sub2" >/dev/null
  echo "n=${#DIRSTACK[@]}"
  DIRSTACK[1]=/; echo "rc=$? [${DIRSTACK[1]}]"
  dirs +1
  popd >/dev/null; echo "pwd=$PWD" )

echo "=== element 0 is the current directory, and writing it is ignored"
DIRSTACK[0]=/nowhere; echo "rc=$? same=$([ "${DIRSTACK[0]}" = "$PWD" ] && echo yes || echo no) pwd=$([ "$PWD" = "$ROOT" ] && echo unmoved)"

echo "=== an element past the end is dropped"
DIRSTACK[5]=q; echo "rc=$? n=${#DIRSTACK[@]} [${DIRSTACK[5]-unset}]"
DIRSTACK+=(zz); echo "rc=$? n=${#DIRSTACK[@]}"

echo "=== a whole-array assignment is elementwise, not a replacement"
( pushd "$ROOT/../sub2" >/dev/null
  DIRSTACK=(a b c); echo "rc=$? n=${#DIRSTACK[@]} [${DIRSTACK[1]}]" )
DIRSTACK=(a b c); echo "rc=$? n=${#DIRSTACK[@]} same=$([ "${DIRSTACK[0]}" = "$PWD" ] && echo yes || echo no)"

echo "=== unsetting an element changes nothing; the value function refills it"
( pushd "$ROOT/../sub2" >/dev/null
  unset 'DIRSTACK[1]'; echo "rc=$? n=${#DIRSTACK[@]}"
  unset 'DIRSTACK[0]'; echo "rc=$? n=${#DIRSTACK[@]}" )

echo "=== every other write path goes through the same hook"
( pushd "$ROOT/../sub2" >/dev/null; printf -v 'DIRSTACK[1]' /etc; echo "rc=$? [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; read -r 'DIRSTACK[1]' <<</etc; echo "rc=$? [${DIRSTACK[1]}]" )
# A `read`'s *later* names are stored down a different path from its first.
( pushd "$ROOT/../sub2" >/dev/null; read -r j 'DIRSTACK[1]' <<<'a /etc'; echo "rc=$? [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; let 'DIRSTACK[1] = 5'; echo "rc=$? [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; read -a DIRSTACK <<<'p q'; echo "rc=$? n=${#DIRSTACK[@]} [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; mapfile -t DIRSTACK <<<$'p\nq'; echo "rc=$? n=${#DIRSTACK[@]} [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; (( DIRSTACK[1] = 5 )); echo "rc=$? [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; declare 'DIRSTACK[1]'=/etc; echo "rc=$? [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; declare DIRSTACK=(x y z); echo "rc=$? n=${#DIRSTACK[@]} [${DIRSTACK[1]}]" )
( pushd "$ROOT/../sub2" >/dev/null; for DIRSTACK in q r; do :; done; echo "rc=$? n=${#DIRSTACK[@]}" )
# The value is stored as written — no tilde expansion, no path resolution —
# which `dirs` then renders as it would any other entry.
( pushd "$ROOT/../sub2" >/dev/null; DIRSTACK[1]='~/zz'; echo "[${DIRSTACK[1]}]"; dirs +1 )

echo "=== readonly is refused loudly, as for any other array"
( readonly DIRSTACK; DIRSTACK[1]=x; echo "rc=$?" ) 2>&1 | sed 's/^.*: //'

echo "=== after unset the name is ordinary and pushd stops touching it"
( unset DIRSTACK; echo "rc=$? [${DIRSTACK[@]-unset}]"
  DIRSTACK=(a b c); echo "n=${#DIRSTACK[@]} [${DIRSTACK[*]}]"
  pushd "$ROOT/../sub2" >/dev/null; echo "[${DIRSTACK[*]}]"
  dirs | wc -l )
# …and only in the shell that unset it.
( unset DIRSTACK ); echo "n=${#DIRSTACK[@]}"

echo "=== a local shadow is ordinary for the length of the call"
( f() { local DIRSTACK=(a b); cd "$ROOT/../sub2"; echo "n=${#DIRSTACK[@]} [${DIRSTACK[*]}]"; }
  f; echo "after n=${#DIRSTACK[@]} same=$([ "${DIRSTACK[0]}" = "$PWD" ] && echo yes || echo no)" )
( f() { local DIRSTACK; pushd "$ROOT/../sub2" >/dev/null; }
  f; echo "n=${#DIRSTACK[@]} same=$([ "${DIRSTACK[0]}" = "$PWD" ] && echo yes || echo no)" )
# `-g` names the global, which is still the dynamic one.
( pushd "$ROOT/../sub2" >/dev/null
  f() { declare -g DIRSTACK=(x y z); }; f; echo "n=${#DIRSTACK[@]} [${DIRSTACK[1]}]" )

echo "=== declare -p and the export attribute"
declare -p DIRSTACK | sed 's/=(.*)/=(...)/'
( f() { export DIRSTACK=(x y); }; f; echo "n=${#DIRSTACK[@]}"; declare -p DIRSTACK ) 2>&1 | sed 's/=(.*)/=(...)/'
