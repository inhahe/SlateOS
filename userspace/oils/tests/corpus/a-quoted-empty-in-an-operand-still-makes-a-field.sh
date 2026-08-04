# `${x:-'' ''}` is two arguments, not one. The operand is expanded where the
# substitution stands, and the word around it then splits whatever it produced
# — by which time the operand's own `''` is gone. Something has to survive to
# say the field was there, and bash spells it `CTLNUL`: a *mark* that is not a
# delimiter, is not removed before the scan, makes its field exist, and
# contributes no text. Quote removal drops it afterwards, so nothing else in
# the shell ever sees one.
#
# Every quoted stretch that produced no character leaves one — `''`, `""`,
# `"$unset"`, `"$(true)"`, `$''`, `$""` — and so does every empty *element* of a
# quoted list. An empty list is not one: `"${e[@]}"` produced no field to be
# empty. An unquoted empty expansion is not one either.
#
# `${x:=w}` is the exception that proves it: the operator answers with the
# variable it just assigned, and that value went through quote removal on the
# way in, so `${t:='' ''}` assigns one space and then splits it away to nothing.

show() { printf '  %-22s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }
e=(); f=(''); g=('' '')
u=; unset n

echo "### the mark makes the field the quotes asked for"
show two          ${nope:-'' ''}
show mid          ${nope:-'' x ''}
show one          ${nope:-''}
show lead         ${nope:-'' x}
show trail        ${nope:-x ''}

echo "### which quoted stretches leave one"
show sq-empty     ${nope:-'' X}
show dq-empty     ${nope:-"" X}
show dq-unset     ${nope:-"$n" X}
show dq-null      ${nope:-"$u" X}
show dq-cmdsub    ${nope:-"$(true)" X}
show dollar-sq    ${nope:-$'' X}
show dollar-dq    ${nope:-$"" X}
show ansi-real    ${nope:-$'\t' X}

echo "### a list answers element by element"
show empty-arr    ${nope:-"${e[@]}" X}
show one-empty    ${nope:-"${f[@]}" X}
show two-empty    ${nope:-"${g[@]}" X}
show star-empty   ${nope:-"${e[*]}" X}
show star-two     ${nope:-"${g[*]}" X}

echo "### an unquoted empty leaves none"
show bare-unset   ${nope:-$n X}
show bare-null    ${nope:-$u X}
show bare-cmdsub  ${nope:-$(true) X}

echo "### two marks in a row are still one field"
show two-sq       ${nope:-'''' X}
show sq-then-dq   ${nope:-''"" X}
show in-a-word    ${nope:-a''b X}

echo "### only where the enclosing word splits"
IFS=: ;  show ifs-colon ${nope:-'' ''}
IFS=' '; show ifs-space ${nope:-'' ''}
IFS=;    show ifs-null  ${nope:-'' ''}
unset IFS
show quoted       "${nope:-'' ''}"

echo "### the other operators that take a word"
s=v
show plus         ${s:+'' ''}
show plus-mid     ${s:+'' x ''}
unset t; show assign ${t:='' ''}
echo "  t=[$t] len=${#t}"

echo "### and through a nested one"
show nested       ${nope:-${also:-''} X}
show nested-q     ${nope:-"${also:-}" X}

echo "### quote removal drops it everywhere else"
v=${nope:-''x''}
echo "  assign  [$v] len=${#v}"
w=abc
echo "  strip   [${w#${nope:-''a''}}]"
echo "  repl    [${w/${nope:-''b''}/X}]"
[[ abc == ${nope:-''a*''} ]] && echo "  cond    match" || echo "  cond    no"
mkdir -p g && : > g/aa && : > g/ab
show glob-lead    ${nope:-''g/a*}
show glob-trail   ${nope:-g/a*''}
show glob-mid     ${nope:-g/a''a}

echo "### and it is not a field break a list made"
a=(p q)
show list-in-op   ${nope:-A"${a[@]}"B}
echo done
