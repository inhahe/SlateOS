# A `[[ ]]` operand and a `case` word/pattern are a joined context of their own
# (bash calls it `W_NOSPLIT2`), and the `[@]` half of it never consults $IFS:
# every list an operator derived — a trim, a `@Q`, a case fold, a replacement, a
# slice, a key list, a prefix-name list, the positionals — glues with a *space*,
# quoted or not. The `[*]` half is unchanged: it still joins with $IFS's first
# character, and with nothing when $IFS is null. Every other joined context — an
# assignment's value, a `declare`, a here-string — keeps $IFS for both halves.

declare -a n=(ax by cz)
declare -a one=(solo)
declare -a e=()
declare -A m=([k1]=v1 [k2]=v2)
preA=1
preB=2
SAVE=$IFS

set -- p q

for lbl in default colon null; do
  echo "=== IFS $lbl"
  case $lbl in
    default) IFS=$SAVE ;;
    colon) IFS=: ;;
    null) IFS= ;;
  esac

  # `[[ ]]` swallows the word it tests, so capture it with a regex instead.
  [[ ${n[@]}      =~ ^(.*)$ ]] && echo "  \${n[@]}       <${BASH_REMATCH[1]}>"
  [[ ${n[*]}      =~ ^(.*)$ ]] && echo "  \${n[*]}       <${BASH_REMATCH[1]}>"
  [[ ${n[@]#a}    =~ ^(.*)$ ]] && echo "  \${n[@]#a}     <${BASH_REMATCH[1]}>"
  [[ ${n[*]#a}    =~ ^(.*)$ ]] && echo "  \${n[*]#a}     <${BASH_REMATCH[1]}>"
  [[ ${n[@]%z}    =~ ^(.*)$ ]] && echo "  \${n[@]%z}     <${BASH_REMATCH[1]}>"
  [[ ${n[@]@Q}    =~ ^(.*)$ ]] && echo "  \${n[@]@Q}     <${BASH_REMATCH[1]}>"
  [[ ${n[*]@Q}    =~ ^(.*)$ ]] && echo "  \${n[*]@Q}     <${BASH_REMATCH[1]}>"
  [[ ${n[@]^^}    =~ ^(.*)$ ]] && echo "  \${n[@]^^}     <${BASH_REMATCH[1]}>"
  [[ ${n[@]/a/Q}  =~ ^(.*)$ ]] && echo "  \${n[@]/a/Q}   <${BASH_REMATCH[1]}>"
  [[ ${n[@]//[ac]/Q} =~ ^(.*)$ ]] && echo "  \${n[@]//../Q} <${BASH_REMATCH[1]}>"
  [[ ${n[@]:0:2}  =~ ^(.*)$ ]] && echo "  \${n[@]:0:2}   <${BASH_REMATCH[1]}>"
  [[ ${n[*]:0:2}  =~ ^(.*)$ ]] && echo "  \${n[*]:0:2}   <${BASH_REMATCH[1]}>"
  [[ ${n[@]:-d}   =~ ^(.*)$ ]] && echo "  \${n[@]:-d}    <${BASH_REMATCH[1]}>"
  [[ ${n[*]:-d}   =~ ^(.*)$ ]] && echo "  \${n[*]:-d}    <${BASH_REMATCH[1]}>"
  [[ ${!n[@]}     =~ ^(.*)$ ]] && echo "  \${!n[@]}      <${BASH_REMATCH[1]}>"
  [[ ${!n[*]}     =~ ^(.*)$ ]] && echo "  \${!n[*]}      <${BASH_REMATCH[1]}>"
  [[ ${!m[@]}     =~ ^(.*)$ ]] && echo "  \${!m[@]}      <${BASH_REMATCH[1]}>"
  [[ ${m[@]#v}    =~ ^(.*)$ ]] && echo "  \${m[@]#v}     <${BASH_REMATCH[1]}>"
  [[ ${!pre@}     =~ ^(.*)$ ]] && echo "  \${!pre@}      <${BASH_REMATCH[1]}>"
  [[ ${!pre*}     =~ ^(.*)$ ]] && echo "  \${!pre*}      <${BASH_REMATCH[1]}>"
  [[ ${@}         =~ ^(.*)$ ]] && echo "  \$@            <${BASH_REMATCH[1]}>"
  [[ ${*}         =~ ^(.*)$ ]] && echo "  \$*            <${BASH_REMATCH[1]}>"
  [[ ${@#p}       =~ ^(.*)$ ]] && echo "  \${@#p}        <${BASH_REMATCH[1]}>"
  [[ ${*#p}       =~ ^(.*)$ ]] && echo "  \${*#p}        <${BASH_REMATCH[1]}>"
  [[ ${@:1:2}     =~ ^(.*)$ ]] && echo "  \${@:1:2}      <${BASH_REMATCH[1]}>"
  [[ ${@:-d}      =~ ^(.*)$ ]] && echo "  \${@:-d}       <${BASH_REMATCH[1]}>"

  # Quoting does not put $IFS back for the `[@]` half — but it does for `[*]`.
  [[ "${n[@]#a}"   =~ ^(.*)$ ]] && echo "  \"\${n[@]#a}\"   <${BASH_REMATCH[1]}>"
  [[ "${n[*]#a}"   =~ ^(.*)$ ]] && echo "  \"\${n[*]#a}\"   <${BASH_REMATCH[1]}>"
  [[ "${n[@]@Q}"   =~ ^(.*)$ ]] && echo "  \"\${n[@]@Q}\"   <${BASH_REMATCH[1]}>"
  [[ "${n[@]:0:2}" =~ ^(.*)$ ]] && echo "  \"\${n[@]:0:2}\" <${BASH_REMATCH[1]}>"
  [[ "${!n[@]}"    =~ ^(.*)$ ]] && echo "  \"\${!n[@]}\"    <${BASH_REMATCH[1]}>"
  [[ "${@#p}"      =~ ^(.*)$ ]] && echo "  \"\${@#p}\"      <${BASH_REMATCH[1]}>"

  # A one-element and an empty array have no separator to show, but they do have
  # a field count: the joined context keeps them one word either way.
  [[ ${one[@]#s}  =~ ^(.*)$ ]] && echo "  \${one[@]#s}   <${BASH_REMATCH[1]}>"
  [[ ${e[@]#s}    =~ ^(.*)$ ]] && echo "  \${e[@]#s}     <${BASH_REMATCH[1]}>"
  [[ -z ${e[@]#s} ]] && echo "  \${e[@]#s}     empty"

  # The subject of a `case`, and its patterns, are the same context.
  case ${n[@]#a} in
    'x by cz') echo "  case \${n[@]#a} space" ;;
    'x:by:cz') echo "  case \${n[@]#a} colon" ;;
    *) echo "  case \${n[@]#a} other" ;;
  esac
  case ${n[*]#a} in
    'x by cz') echo "  case \${n[*]#a} space" ;;
    'x:by:cz') echo "  case \${n[*]#a} colon" ;;
    'xbycz') echo "  case \${n[*]#a} glued" ;;
    *) echo "  case \${n[*]#a} other" ;;
  esac
  case 'x by cz' in
    ${n[@]#a}) echo "  pat  \${n[@]#a} space" ;;
    *) echo "  pat  \${n[@]#a} other" ;;
  esac
  case '0 1 2' in
    ${!n[@]}) echo "  pat  \${!n[@]}  space" ;;
    *) echo "  pat  \${!n[@]}  other" ;;
  esac

  # The other joined contexts keep $IFS for both halves.
  a=${n[@]#a};   echo "  a=\${n[@]#a}   [$a]"
  a=${n[*]#a};   echo "  a=\${n[*]#a}   [$a]"
  a="${n[@]:0:2}"; echo "  a=\"..:0:2\"    [$a]"
  declare D=${n[@]#a}; echo "  declare       [$D]"
  printf '  <<<           '; cat <<< ${n[@]#a}
done
IFS=$SAVE
