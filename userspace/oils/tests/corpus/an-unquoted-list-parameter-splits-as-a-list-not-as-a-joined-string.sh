# Unquoted, a whole-array reference is a *list*, and the `[*]` spelling says
# nothing — the star only chooses a separator inside double quotes. With a
# non-null $IFS that list joins with $IFS's first character and splits again;
# with a null $IFS nothing can split, so the list itself is the fields.

declare -a n=(x y z)
declare -a e=()
declare -A m=([k1]=v1 [k2]=v2)
preA=1; preB=2
r='n[@]'
set -- p q r
SAVE=$IFS

# The field *count* comes first, so "no fields at all" is told from "one empty
# field" — which `printf '<%s>'` alone would print the same way.
show() { printf '  %-14s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

for lbl in default colon null space-colon; do
  echo "=== IFS $lbl"
  case $lbl in
    default) IFS=$SAVE ;;
    colon) IFS=: ;;
    null) IFS= ;;
    space-colon) IFS=' :' ;;
  esac
  show '${n[@]}' ${n[@]}
  show '${n[*]}' ${n[*]}
  show '${!n[@]}' ${!n[@]}
  show '${!n[*]}' ${!n[*]}
  show '${n[@]:0:2}' ${n[@]:0:2}
  show '${n[*]:0:2}' ${n[*]:0:2}
  show '${n[@]#x}' ${n[@]#x}
  show '${n[*]#x}' ${n[*]#x}
  show '${n[@]/x/Q}' ${n[@]/x/Q}
  show '${n[@]^^}' ${n[@]^^}
  show '${n[@]@Q}' ${n[@]@Q}
  show '${n[@]@a}' ${n[@]@a}
  show '${n[@]@A}' ${n[@]@A}
  show '${n[@]@k}' ${n[@]@k}
  show '${n[@]:-d}' ${n[@]:-d}
  show '${e[@]:-d}' ${e[@]:-d}
  show '${m[@]}' ${m[@]}
  show '${!m[@]}' ${!m[@]}
  show '${!m[*]}' ${!m[*]}
  show '$@' $@
  show '$*' $*
  show '${@:1:2}' ${@:1:2}
  show '${@#p}' ${@#p}
  show '${*#p}' ${*#p}
  show '${!pre@}' ${!pre@}
  show '${!pre*}' ${!pre*}
  show '${!r}' ${!r}
  show 'A${n[@]}B' A${n[@]}B
  show 'A${n[*]}B' A${n[*]}B
done
IFS=$SAVE

echo "=== elements holding IFS characters"
declare -a c=('a:b' 'c d' 'e')
IFS=:; show '${c[@]}' ${c[@]}; show '${c[*]}' ${c[*]}
IFS=' '; show '${c[@]}' ${c[@]}; show '${c[*]}' ${c[*]}
IFS=; show '${c[@]}' ${c[@]}; show '${c[*]}' ${c[*]}
IFS=$SAVE

echo "=== empty elements open no field of their own"
declare -a z=('' 'q' '')
IFS=:; show '${z[@]}' ${z[@]}; show '${z[*]}' ${z[*]}
IFS=; show '${z[@]}' ${z[@]}; show '${z[*]}' ${z[*]}
IFS=; show 'A${z[@]}B' A${z[@]}B
IFS=$SAVE

echo "=== a scalar, a substitution and an assignment are not lists"
s='a b'
IFS=; show '$s' $s
IFS=; show '$(echo a b)' $(echo a b)
IFS=:; v=${n[@]}; echo "  assign [$v]"
IFS=; v=${n[@]}; echo "  assign [$v]"
IFS=:; v=${n[@]@Q}; echo "  assign [$v]"
IFS=; v=${n[@]@Q}; echo "  assign [$v]"
IFS=$SAVE
