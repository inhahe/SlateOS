# Where a word cannot become more than one field — an assignment's value, a
# `[[ ]]`/`case` word or pattern, a here-string — an unquoted list still has to
# collapse to a string, and which separator it collapses with is *not* uniform.
# `${a[@]:off:len}` and `${!a[@]}` collapse with a **space** whatever $IFS says,
# while `${a[@]}` itself, `$@`, every `[*]` spelling, `${a[@]@Q}`, `${a[@]#p}`
# and `${!prefix@}` all obey $IFS. Quoting the very same slice flips it back to
# $IFS, so the space is a property of the *context*, not of the expansion.

declare -a n=(x y z)
declare -A m=([k1]=v1 [k2]=v2)
preA=1; preB=2
set -- p q r
SAVE=$IFS

for lbl in default colon null; do
  echo "=== IFS $lbl"
  case $lbl in
    default) IFS=$SAVE ;;
    colon) IFS=: ;;
    null) IFS= ;;
  esac

  # An assignment's value, in all the spellings that reach one.
  a=${n[@]:0:2};    echo "  a=\${n[@]:0:2}   [$a]"
  a=${n[*]:0:2};    echo "  a=\${n[*]:0:2}   [$a]"
  a=${!n[@]};       echo "  a=\${!n[@]}      [$a]"
  a=${!n[*]};       echo "  a=\${!n[*]}      [$a]"
  a=${!m[@]};       echo "  a=\${!m[@]}      [$a]"
  a=${@:1:2};       echo "  a=\${@:1:2}      [$a]"
  a=${*:1:2};       echo "  a=\${*:1:2}      [$a]"
  a=${n[@]};        echo "  a=\${n[@]}       [$a]"
  a=${n[@]@Q};      echo "  a=\${n[@]@Q}     [$a]"
  a=${n[@]#x};      echo "  a=\${n[@]#x}     [$a]"
  a=${!pre@};       echo "  a=\${!pre@}      [$a]"
  a="${n[@]:0:2}";  echo "  a=\"\${n[@]:0:2}\" [$a]"
  a="${!n[@]}";     echo "  a=\"\${!n[@]}\"    [$a]"
  a=A${n[@]:0:2}B;  echo "  a=A\${n[@]:0:2}B [$a]"

  declare -a t=(- -)
  t[0]=${n[@]:0:2};  echo "  t[0]=            [${t[0]}]"
  export E=${n[@]:0:2};  echo "  export E=        [$E]"
  declare D=${!n[@]};    echo "  declare D=       [$D]"
  local_probe() { local L=${n[@]:0:2}; echo "  local L=         [$L]"; }
  local_probe
  readonly_probe() ( readonly R=${n[@]:0:2}; echo "  readonly R=      [$R]" )
  readonly_probe

  # A `[[ ]]`/`case` word, and the same text used as a pattern.
  [[ ${n[@]:0:2} == 'x y' ]] && echo "  [[ word ]]       space" || echo "  [[ word ]]       ifs"
  [[ ${!n[@]} == '0 1 2' ]] && echo "  [[ keys ]]       space" || echo "  [[ keys ]]       ifs"
  [[ ${n[@]} == 'x y z' ]] && echo "  [[ whole ]]      space" || echo "  [[ whole ]]      ifs"
  case ${n[@]:0:2} in 'x y') echo "  case word        space" ;; *) echo "  case word        ifs" ;; esac
  case 'x y' in ${n[@]:0:2}) echo "  case pattern     space" ;; *) echo "  case pattern     ifs" ;; esac
  [[ 'x y' == ${n[@]:0:2} ]] && echo "  [[ pattern ]]    space" || echo "  [[ pattern ]]    ifs"
  case '0 1 2' in ${!n[@]}) echo "  case key pattern space" ;; *) echo "  case key pattern ifs" ;; esac

  # A here-string is one word too.
  printf '  <<< slice        '; cat <<< ${n[@]:0:2}
  printf '  <<< keys         '; cat <<< ${!n[@]}
  printf '  <<< whole        '; cat <<< ${n[@]}
done
IFS=$SAVE

echo "=== the space is the join, not a rewrite of the elements"
declare -a c=('a b' 'c:d' e)
IFS=:; a=${c[@]:0:2}; echo "  colon [$a]"
IFS=; a=${c[@]:0:2}; echo "  null  [$a]"
IFS=$SAVE; a=${c[@]:0:2}; echo "  deflt [$a]"

echo "=== an empty or one-element slice joins nothing"
declare -a z=('' q '')
IFS=:; a=${z[@]:0:2}; echo "  z 0 2 [$a]"
IFS=:; a=${z[@]:1:1}; echo "  z 1 1 [$a]"
IFS=:; a=${z[@]:9:2}; echo "  z 9 2 [$a]"
IFS=:; a=${z[@]:0:0}; echo "  z 0 0 [$a]"
IFS=$SAVE
