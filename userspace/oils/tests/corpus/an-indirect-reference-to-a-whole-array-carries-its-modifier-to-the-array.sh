# `${!ref<op>}` resolves `ref` to a *name* and applies the modifier to it. When
# that name is a whole-array reference — `ref='n[@]'` — the modifier is the
# array's own, so `${!ref#a}` is `${n[@]#a}` down to the last field: one result
# per element, `:off:len` a slice of elements rather than a substring, and the
# `[@]`/`[*]` distinction alive inside quotes. A nameref is the one exception,
# and only for the `${x:-…}` family: it answers with the name it holds.

declare -a n=(ax by cz)
declare -a e=()
declare -a p=('n[@]' 'n[*]' 'n[1]')
SAVE=$IFS

show() { printf '  %-14s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

for ref in 'n[@]' 'n[*]' 'n[1]' 'n'; do
  echo "### ref=$ref"
  r=$ref
  for lbl in default colon null; do
    echo "=== IFS $lbl"
    case $lbl in
      default) IFS=$SAVE ;;
      colon) IFS=: ;;
      null) IFS= ;;
    esac
    show '${!r}'       ${!r}
    show '${!r:-d}'    ${!r:-d}
    show '${!r:+A}'    ${!r:+A}
    show '${!r-d}'     ${!r-d}
    show '${!r#a}'     ${!r#a}
    show '${!r##*}'    ${!r##*}
    show '${!r%z}'     ${!r%z}
    show '${!r%%*}'    ${!r%%*}
    show '${!r/a/Q}'   ${!r/a/Q}
    show '${!r//[abc]/Q}' ${!r//[abc]/Q}
    show '${!r^^}'     ${!r^^}
    show '${!r,,}'     ${!r,,}
    show '${!r~~}'     ${!r~~}
    show '${!r@Q}'     ${!r@Q}
    show '${!r@U}'     ${!r@U}
    show '${!r:1}'     ${!r:1}
    show '${!r:1:2}'   ${!r:1:2}
    show '"${!r#a}"'   "${!r#a}"
    show '"${!r:1:2}"' "${!r:1:2}"
    show '"${!r:-d}"'  "${!r:-d}"
    show '"${!r^^}"'   "${!r^^}"
    show '"${!r@Q}"'   "${!r@Q}"
    show 'A${!r#a}B'   A${!r#a}B
    a=${!r};      echo "  a=\${!r}     [$a]"
    a=${!r#a};    echo "  a=\${!r#a}   [$a]"
    a=${!r:1:2};  echo "  a=\${!r:1:2} [$a]"
    a="${!r:1:2}"; echo "  a=\"..\"     [$a]"
  done
  IFS=$SAVE
done

echo "### the pointer may be an array element"
IFS=:
show '${!p[0]#a}'   ${!p[0]#a}
show '${!p[1]#a}'   ${!p[1]#a}
show '${!p[2]#b}'   ${!p[2]#b}
show '"${!p[0]#a}"' "${!p[0]#a}"
show '"${!p[1]#a}"' "${!p[1]#a}"
IFS=$SAVE

echo "### a nameref answers with the name, but only for the :- family"
declare -n g='n[@]'
show '${!g}'     ${!g}
show '${!g:-d}'  ${!g:-d}
show '${!g:+A}'  ${!g:+A}
show '${!g#a}'   ${!g#a}
show '${!g^^}'   ${!g^^}
show '${!g:1:2}' ${!g:1:2}
show '"${!g}"'   "${!g}"
show '"${!g#a}"' "${!g#a}"
unset -n g

echo "### an empty and a missing array"
r='e[@]'
show '${!r:-d}'  ${!r:-d}
show '${!r-d}'   ${!r-d}
show '${!r:+A}'  ${!r:+A}
show '${!r#a}'   ${!r#a}
show '${!r:0:2}' ${!r:0:2}
r='nosuch[@]'
show '${!r:-d}'  ${!r:-d}
show '${!r#a}'   ${!r#a}
show '${!r@Q}'   ${!r@Q}

echo "### the complaint names the reference, not the array"
r='e[@]'
(echo ${!r:?boom}); echo "  rc=$?"
(echo ${!r:?}); echo "  rc=$?"
(echo ${!r?}); echo "  rc=$?"

echo "### assigning through the reference is still a bad subscript"
r='n[@]'
(echo ${!r:=v}); echo "  rc=$?"

# A `[[ ]]` operand and a `case` word are the one context in which the indirect
# spelling and the written-out one part company: there $IFS's first character is
# a space, and only a `[*]` referent's null-$IFS rule survives it. Quoting the
# same reference puts $IFS back.
echo "### a [[ ]] operand and a case word join the reference with a space"
for lbl in default colon null; do
  echo "=== IFS $lbl"
  case $lbl in
    default) IFS=$SAVE ;;
    colon) IFS=: ;;
    null) IFS= ;;
  esac
  r='n[@]'; s='n[*]'
  [[ ${!r}     =~ ^(.*)$ ]] && echo "  \${!r}       <${BASH_REMATCH[1]}>"
  [[ ${!s}     =~ ^(.*)$ ]] && echo "  \${!s}       <${BASH_REMATCH[1]}>"
  [[ ${n[*]}   =~ ^(.*)$ ]] && echo "  \${n[*]}     <${BASH_REMATCH[1]}>"
  [[ "${!r}"   =~ ^(.*)$ ]] && echo "  \"\${!r}\"     <${BASH_REMATCH[1]}>"
  [[ "${!s}"   =~ ^(.*)$ ]] && echo "  \"\${!s}\"     <${BASH_REMATCH[1]}>"
  [[ ${!r#a}   =~ ^(.*)$ ]] && echo "  \${!r#a}     <${BASH_REMATCH[1]}>"
  [[ ${!s#a}   =~ ^(.*)$ ]] && echo "  \${!s#a}     <${BASH_REMATCH[1]}>"
  [[ "${!s#a}" =~ ^(.*)$ ]] && echo "  \"\${!s#a}\"   <${BASH_REMATCH[1]}>"
  [[ ${!r:0:2} =~ ^(.*)$ ]] && echo "  \${!r:0:2}   <${BASH_REMATCH[1]}>"
  [[ ${!s:0:2} =~ ^(.*)$ ]] && echo "  \${!s:0:2}   <${BASH_REMATCH[1]}>"
  [[ ${!r:-d}  =~ ^(.*)$ ]] && echo "  \${!r:-d}    <${BASH_REMATCH[1]}>"
  [[ ${!s:-d}  =~ ^(.*)$ ]] && echo "  \${!s:-d}    <${BASH_REMATCH[1]}>"
  case ${!s} in 'ax by cz') echo "  case \${!s}   space" ;; 'ax:by:cz') echo "  case \${!s}   colon" ;; *) echo "  case \${!s}   other" ;; esac
  case ${n[*]} in 'ax by cz') echo "  case \${n[*]} space" ;; 'ax:by:cz') echo "  case \${n[*]} colon" ;; *) echo "  case \${n[*]} other" ;; esac
  # A command substitution's own words are in whatever context they make.
  f() { local v=${!s}; echo "$v"; }
  [[ $(f) =~ ^(.*)$ ]] && echo "  \$(f)        <${BASH_REMATCH[1]}>"
done
IFS=$SAVE

echo "### the same reference in the contexts that keep \$IFS"
IFS=:
s='n[*]'
a=${!s};       echo "  a=\${!s}   [$a]"
declare D=${!s}; echo "  declare   [$D]"
printf '  <<<       '; cat <<< ${!s}
printf '  split     '; printf '<%s>' ${!s}; echo
IFS=$SAVE
