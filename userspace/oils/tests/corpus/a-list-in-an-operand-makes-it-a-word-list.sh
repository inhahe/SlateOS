# bash expands a word along one of two paths, and a quoted `[@]` list decides
# which. Without one the word is an `istring` — the text as written, with a
# `CTLNUL` wherever a quoted stretch produced no character (the mark of
# `a-quoted-empty-in-an-operand-still-makes-a-field.sh`). With one it is a
# `WORD_LIST`: the list's elements are words, the text between them is
# word-split like any other, and a word carries a `CTLNUL` only when it came out
# wholly empty — the marks the ordinary quoted stretches made are gone.
#
# So in an operand a list changes two things at once. It moves the marks:
#
#   ${x:-''a}            \177a      — one mark per quoted-empty stretch
#   ${x:-''"${f[@]}"a}   a          — the list took it, and `f`'s empty element
#                                     merged into the word `a`, so no mark
#   ${x:-"${g[@]}"a}     \177 a     — `g`'s first element *is* an empty word
#
# and it word-splits what is between:
#
#   ${x:-''  X}          \177  X    — istring: the two spaces are text
#   ${x:-"${f[@]}"  X}   \177 X     — word list: they were a delimiter
#
# Only a *joining* reader sees any of this: the fields are the same either way,
# which is why a command argument cannot tell the two paths apart. And of the
# joining readers, three do the splitting first — a `case` word, a `[[ ]]`
# operand, and any pattern — while three do not: an assignment's RHS
# (`PF_ASSIGNRHS`), a here-document (`Q_HERE_DOCUMENT`), and the inside of
# double quotes (`Q_DOUBLE_QUOTES`). `v=${x:-"${f[@]}"  X}` keeps both spaces.
#
# `[*]` is not a list — it joins its elements into one string — and neither is
# an *unquoted* `[@]`, which is a list only to the splitter.
#
# A `case` subject cannot be printed — quote removal would destroy the very
# marks this is about — so its length is asked of the `case` itself, with a
# pattern of that many `?`. The subject is written out at every one of those
# sites rather than passed to a helper: an argument would have gone through a
# double-quoted expansion, which is one of the contexts that does not split.

f=(''); g=('' ''); h=('' x ''); e=()
L() { printf '  %-24slen=%s\n' "$1" "$2"; }
p() { printf '  %-24s' "$1"; }
b() { printf '%s' "$1" | od -An -tx1 | tr -d '\n'; printf '  len=%s\n' "${#1}"; }

echo "### the marks a list moved"
for q in '' '?' '??' '???'; do case ${nope:-''a} in $q) L 'sq a' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''''a} in $q) L 'sq sq a' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''a"$*"} in $q) L 'sq a star' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''a${e[@]}} in $q) L 'sq a unq-e@' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-"${f[@]}"a} in $q) L 'f@ a' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''"${f[@]}"''a} in $q) L 'sq f@ sq a' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''a"${f[@]}"} in $q) L 'sq a f@' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''a"$@"} in $q) L 'sq a at' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-"${g[@]}"a} in $q) L 'g@ a' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????' '?????'; do case ${nope:-"${h[@]}"a} in $q) L 'h@ a' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''a"${!f[@]}"} in $q) L 'sq a keys-f@' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''a"${g[@]:0:1}"} in $q) L 'sq a slice-g@' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''a"${g[@]#x}"} in $q) L 'sq a bulk-g@' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-${also:-"${f[@]}"}a} in $q) L 'nested' "${#q}"; break;; esac; done

echo "### and the word splitting it brought with it"
for q in '' '?' '??' '???' '????'; do case ${nope:-"${f[@]}"  X} in $q) L 'f@ sp sp X' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????'; do case ${nope:-''  X} in $q) L 'sq sp sp X' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????' '?????'; do case ${nope:-"${f[@]}" X Y} in $q) L 'f@ sp X sp Y' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????'; do case ${nope:-"${e[@]}"  X} in $q) L 'e@ sp sp X' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????'; do case ${nope:-'' "${e[@]}" X} in $q) L 'sq sp e@ sp X' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????' '?????'; do case ${nope:-'' "${f[@]}" X} in $q) L 'sq sp f@ sp X' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''X "${e[@]}"} in $q) L 'sq X sp e@' "${#q}"; break;; esac; done
for q in '' '?' '??' '???'; do case ${nope:-''X "$@"} in $q) L 'sq X sp at' "${#q}"; break;; esac; done
s='a  b'
for q in '' '?' '??' '???' '????' '?????' '??????'; do case ${nope:-"${f[@]}" $s} in $q) L 'f@ sp unq-s' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????' '?????' '??????'; do case ${nope:-"${f[@]}" "$s"} in $q) L 'f@ sp dq-s' "${#q}"; break;; esac; done

echo "### a case matches those bytes"
D=$'\177'
p 'f@ a vs a';       case ${nope:-"${f[@]}"a} in a) echo match;; *) echo no;; esac
p 'sq a vs a';       case ${nope:-''a} in a) echo match;; *) echo no;; esac
p 'g@ a vs D_a';     case ${nope:-"${g[@]}"a} in $D' a') echo match;; *) echo no;; esac
p 'f@ sp sp X D_X';  case ${nope:-"${f[@]}"  X} in $D' X') echo match;; *) echo no;; esac
p 'f@ sp sp X __X';  case ${nope:-"${f[@]}"  X} in '  X') echo match;; *) echo no;; esac
p 'f@ a star';       case abc in ${nope:-"${f[@]}"a*}) echo match;; *) echo no;; esac
p 'e@ a star';       case abc in ${nope:-"${e[@]}"a*}) echo match;; *) echo no;; esac
p 'sq f@ sq a star'; case abc in ${nope:-''"${f[@]}"''a*}) echo match;; *) echo no;; esac
p 'g@ a star';       case abc in ${nope:-"${g[@]}"a*}) echo match;; *) echo no;; esac
p 'g@ a star 2';     case ' abc' in ${nope:-"${g[@]}"a*}) echo match;; *) echo no;; esac

echo "### the joining readers that split first"
[[ ' X' == ${nope:-"${f[@]}"  X} ]] && echo '  cond-rhs   match' || echo '  cond-rhs   no'
[[ '  X' == ${nope:-"${f[@]}"  X} ]] && echo '  cond-rhs2  no-split' || echo '  cond-rhs2  split'
[[ ${nope:-"${f[@]}"  X} == ' X' ]] && echo '  cond-lhs   match' || echo '  cond-lhs   no'
w='  X'; p 'strip';   b "${w#${nope:-"${f[@]}"  X}}"
w='ab';  p 'strip-g'; b "${w#${nope:-"${g[@]}"a}}"
w='ab';  p 'repl';    b "${w/${nope:-"${g[@]}"a}/Z}"

echo "### the joining readers that do not"
v=${nope:-"${f[@]}"  X};   p 'assign f@ sp sp X'; b "$v"
v=${nope:-''  X};          p 'assign sq sp sp X'; b "$v"
v=${nope:-"${g[@]}"a};     p 'assign g@ a';       b "$v"
v=${nope:-''a"${f[@]}"};   p 'assign sq a f@';    b "$v"
v=${nope:-"${f[@]}" X Y};  p 'assign f@ X Y';     b "$v"
declare v2=${nope:-"${f[@]}"  X}; p 'declare';    b "$v2"
p 'here-string'; cat <<< ${nope:-"${f[@]}"  X} | od -An -tx1 | tr -d '\n'; echo
p 'here-doc'; cat <<EOF | od -An -tx1 | tr -d '\n'; echo
${nope:-"${f[@]}"  X}
EOF
p 'dquoted'; b "${nope:-"${f[@]}"  X}"

echo "### the splitting reader cannot tell them apart"
q() { printf '  %-24s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }
q 'sq sp e@ sp X'  ${nope:-'' "${e[@]}" X}
q 'sq sp f@ sp X'  ${nope:-'' "${f[@]}" X}
q 'sq sp X sp e@'  ${nope:-'' X "${e[@]}"}
q 'sq sp X sp at'  ${nope:-'' X "$@"}
q 'sq sp X'        ${nope:-'' X}
q 'sq sp X star'   ${nope:-'' X "$*"}
q 'f@ sp sp X'     ${nope:-"${f[@]}"  X}
q 'g@ a'           ${nope:-"${g[@]}"a}
a2=(${nope:-"${f[@]}"  X}); q 'array' "${a2[@]}"

echo "### under IFS=:"
IFS=:
for q in '' '?' '??' '???' '????'; do case ${nope:-"${f[@]}" X} in $q) L 'f@ sp X' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????'; do case ${nope:-"${f[@]}":X} in $q) L 'f@ colon X' "${#q}"; break;; esac; done
for q in '' '?' '??' '???' '????'; do case ${nope:-''  X} in $q) L 'sq sp sp X' "${#q}"; break;; esac; done
v=${nope:-"${f[@]}":X}; p 'assign f@ colon X'; b "$v"
v=${nope:-"${f[@]}" X}; p 'assign f@ sp X';    b "$v"
unset IFS
echo done
