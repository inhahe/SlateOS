# `${@:-d}` is not `${x:-d}` on a joined `$@`: bash answers the use/alternate
# family on the positionals with the positional *list*, one field per parameter,
# exactly as `${a[@]:-d}` answers with an array's elements. `set -- p q r` makes
# `${@:-d}` three fields under every `$IFS`, quoted or not — only `"${*:-d}"`
# joins, because that is what a quoted star means.
#
# The two parameters are the same case, so what is left to tell apart is not the
# operator but the parameter: there is no variable behind `$@` to assign a
# default to, and a complaint about it is spelled `@` with no subscript. Note
# also that `$0` does *not* head the list here, though it does for a slice.

show() { printf '  %-14s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

echo "### three positionals, under three separators"
for ifs in ' ' ':' ''; do
  IFS=$ifs
  ( set -- p q r
    printf '  [%s]\n' "$ifs"
    show '${@:-d}'   ${@:-d}
    show '${*:-d}'   ${*:-d}
    show '${@-d}'    ${@-d}
    show '${*-d}'    ${*-d}
    show 'q ${@:-d}' "${@:-d}"
    show 'q ${*:-d}' "${*:-d}"
    show 'q ${@-d}'  "${@-d}"
    show '${@:+A}'   ${@:+A}
    show 'q ${@:+A}' "${@:+A}" )
  IFS=' '
done
IFS=' '

echo "### and with nothing to hand back"
( set --
  show '${@:-d}'   ${@:-d}
  show '${*:-d}'   ${*:-d}
  show '${@-d}'    ${@-d}
  show 'q ${@:-d}' "${@:-d}"
  show '${@:+A}'   ${@:+A}
  show 'q ${@:+A}' "${@:+A}"
  show '${@+A}'    ${@+A} )

echo "### one empty positional is null but not unset"
( set -- ""
  show '${@:-d}'   ${@:-d}
  show 'q ${@:-d}' "${@:-d}"
  show '${@-d}'    ${@-d}
  show 'q ${@-d}'  "${@-d}"
  show '${@:+A}'   ${@:+A}
  show 'q ${@:+A}' "${@:+A}"
  show '${@+A}'    ${@+A} )

echo "### two empty ones are not even null"
( set -- "" ""
  show '${@:-d}'   ${@:-d}
  show 'q ${@:-d}' "${@:-d}"
  show 'q ${*:-d}' "${*:-d}"
  show '${@:+A}'   ${@:+A} )

echo "### the operand is a word like any other"
( set --
  show 'unq'   ${@:-a b c}
  show 'q'     "${@:-a b c}"
  show 'nested' ${@:-${@:-x}} )

echo "### the count, where a joined answer would show"
( set -- p q r; set -- ${@:-d};   echo "  unq  \$#=$#" )
( set -- p q r; set -- "${@:-d}"; echo "  q    \$#=$#" )
( set -- p q r; set -- "${*:-d}"; echo "  star \$#=$#" )
( set -- p q r; set -- ${@-d};    echo "  nc   \$#=$#" )
( set -- "a b" c; set -- "${@:-d}"; echo "  gap  \$#=$# 1=<$1>" )

echo "### \$0 heads a slice, but not this"
( set -- p; show 'slice ${@:0}' ${@:0}; show 'default'  ${@:-d} )

echo "### there is nowhere to put a default"
k=0
( set --; echo "[${@:=d}]" );          echo "  rc=$?"
( set --; echo "[${*:=d}]" );          echo "  rc=$?"
( set --; echo "[${@=d}]" );           echo "  rc=$?"
( set -- p q; echo "[${@:=d}]" );      echo "  rc=$?"
# The operand is not expanded at all: an array's bad-subscript complaint runs a
# command substitution in the word first, this one never gets that far.
( set --; echo "[${@:=$(echo ran >&2; echo v)}]" ); echo "  rc=$?"

echo "### and a complaint carries no subscript"
( set --; echo "[${@:?msg}]" );  echo "  rc=$?"
( set --; echo "[${*:?msg}]" );  echo "  rc=$?"
( set --; echo "[${@:?}]" );     echo "  rc=$?"
( set --; echo "[${@?}]" );      echo "  rc=$?"
( set -- ""; echo "[${@:?msg}]" ); echo "  rc=$?"
( set -- ""; echo "[${@?msg}]" );  echo "  rc=$?"
( set -- p; echo "[${@:?msg}]" );  echo "  rc=$?"

echo "### an unset shell is still not an unset parameter"
( set -u; set --; echo "[${@:-d}]" ); echo "  rc=$?"
( set -u; set --; echo "[${@-d}]" );  echo "  rc=$?"
