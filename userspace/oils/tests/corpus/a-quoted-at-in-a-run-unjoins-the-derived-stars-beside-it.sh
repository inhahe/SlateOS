# A quoted `[@]` anywhere in a double-quoted run stops the *derived* `[*]` forms
# in that same run from joining: they keep one field per item instead. bash
# builds those lists by gluing their items with an **unquoted** `$IFS[0]`, the
# items themselves quote-protected, and then splits the finished word if and only
# if it met a quoted `[@]` while expanding it — so the separator it just wrote is
# the thing that comes apart, and the items are not.
#
# Which is why the rule looks arbitrary and is not:
#
#   * it is the *derived* stars only — the keys, the prefixed names, a slice and
#     an element-wise transform. A plain `${a[*]}`/`$*` is exempt: its separator
#     is as much the value as the elements are. So is `${a[*]:-w}`, which joins
#     elements and whose operand is a word with fields of its own.
#   * order is nothing, since the flag is the whole run's.
#   * the **run** is the scope, not the word: two runs in one word are two
#     separate `expand_word_internal` calls to bash and do not see each other.
#   * an *unquoted* `[@]` does not count. It is `quoted_dollar_at`, not any `[@]`.
#   * an `[@]` with no elements counts. Meeting the spelling is the whole test.
#   * a **null** `$IFS` turns it off: there is no separator to write, so nothing
#     is left for the split to find and the items run together.
#   * a context that never splits — an assignment, a redirect target — keeps the
#     star joined for the same reason.
#   * an operand is not a run of its own. The `[@]` may be outside it or inside
#     it, however deeply, and either way it is this run that unjoins.

declare -a n=('a:b' 'c d' e)
declare -a e=()
declare -A h=([k]='a:b')
pre_x=1; pre_y=2
set -- 'a:b' 'c d' e

p() { printf '%-40s count=%s' "$1" "$(($# - 1))"; shift; printf ' <%s>' "$@"; echo; }

echo "=== which star forms unjoin — alone, then with a quoted [@] in the run"
IFS=:
p '${n[*]@Q}'          "${n[*]@Q}"
p '${n[*]@Q} + [@]'    "${n[*]@Q} ${n[@]}"
p '${n[*]@A}'          "${n[*]@A}"
p '${n[*]@A} + [@]'    "${n[*]@A} ${n[@]}"
p '${n[*]^^}'          "${n[*]^^}"
p '${n[*]^^} + [@]'    "${n[*]^^} ${n[@]}"
p '${n[*]#a}'          "${n[*]#a}"
p '${n[*]#a} + [@]'    "${n[*]#a} ${n[@]}"
p '${n[*]%e}'          "${n[*]%e}"
p '${n[*]%e} + [@]'    "${n[*]%e} ${n[@]}"
p '${n[*]/a/X}'        "${n[*]/a/X}"
p '${n[*]/a/X} + [@]'  "${n[*]/a/X} ${n[@]}"
p '${!n[*]}'           "${!n[*]}"
p '${!n[*]} + [@]'     "${!n[*]} ${n[@]}"
p '${!pre*}'           "${!pre*}"
p '${!pre*} + [@]'     "${!pre*} ${n[@]}"
p '${n[*]:0:2}'        "${n[*]:0:2}"
p '${n[*]:0:2} + [@]'  "${n[*]:0:2} ${n[@]}"
p '${*:1:2} + [@]'     "${*:1:2} ${n[@]}"
p '${*#a} + [@]'       "${*#a} ${n[@]}"
p '${h[*]@Q} + [@]'    "${h[*]@Q} ${n[@]}"

echo "=== …and the forms that stay joined however loudly the [@] shouts"
p '${n[*]}'            "${n[*]}"
p '${n[*]} + [@]'      "${n[*]} ${n[@]}"
p '$* + [@]'           "$* ${n[@]}"
p '${n[*]:-D} + [@]'   "${n[*]:-D} ${n[@]}"
p '${e[*]:-D} + [@]'   "${e[*]:-D} ${n[@]}"
p '${#n[*]} + [@]'     "${#n[*]} ${n[@]}"
p '${h[*]} + [@]'      "${h[*]} ${n[@]}"

echo "=== order is nothing, but the run is everything"
p 'star then [@]'      "${n[*]@Q} ${n[@]}"
p '[@] then star'      "${n[@]} ${n[*]@Q}"
p 'two runs'           "${n[*]@Q}""${n[@]}"
p 'two runs, x between' "${n[*]@Q}"x"${n[@]}"
p 'two runs, other way' "${n[@]}""${n[*]@Q}"
p 'two words'          "${n[*]@Q}" "${n[@]}"
p 'unquoted [@] in run' "${n[*]@Q}"${n[@]}
p 'unquoted $@ in run'  "${n[*]@Q}"$@

echo "=== an [@] with nothing in it still counts"
p 'empty array'        "${n[*]@Q} ${e[@]}"
p 'empty keys'         "${n[*]@Q} ${!e[@]}"
p 'empty slice'        "${n[*]@Q} ${n[@]:9:1}"
( set --; IFS=:; p 'no positionals' "${n[*]@Q} $@" )

echo "=== every \$IFS, and the null one that turns it all off"
for i in ':' '' ' ' ',' ': ' 'a'; do
  ( IFS=$i; p "IFS=[$i]" "${n[*]@Q} ${n[@]}" )
done
( unset IFS; p 'IFS unset' "${n[*]@Q} ${n[@]}" )

echo "=== a context that does not split keeps the star joined"
IFS=:
v="${n[*]@Q} ${n[@]}"; printf 'assign <%s>\n' "$v"
declare -a w=("${n[*]@Q} ${n[@]}"); p 'array literal' "${w[@]}"
printf 'redirect '; cat < /dev/null > "$(printf %s /dev/null)"; echo ok
case "${n[*]@Q} ${n[@]}" in
  *"'c d'"*) echo 'case word: joined text still holds the quoted element' ;;
  *) echo 'case word: something split' ;;
esac

echo "=== an operand is not a run of its own"
p 'star in operand'    "${x:-${n[*]@Q}} ${n[@]}"
p 'star quoted inside' "${x:-"${n[*]@Q}"} ${n[@]}"
p '[@] in operand'     "${n[*]@Q} ${x:-${n[@]}}"
p '[@] quoted inside'  "${n[*]@Q} ${x:-"${n[@]}"}"
p 'nested operands'    "${n[@]} ${x:-${y:-${n[*]@Q}}}"
p 'array-op operand'   "${n[@]} ${e[*]:-${n[*]@Q}}"
p 'unquoted operand'   "${n[@]} "${x:-${n[*]@Q}}

echo "=== the same rule reaches for, [[ ]] and case"
for word in "${n[*]@Q} ${n[@]}"; do printf '[%s]' "$word"; done; echo
for word in "${n[*]@Q}"; do printf '[%s]' "$word"; done; echo
[[ "P:Q${n[*]@Q} ${n[@]}" =~ ^(.*)$ ]] && printf 'cond <%s>\n' "${BASH_REMATCH[1]}"
[[ "P:Q${n[*]@Q}" =~ ^(.*)$ ]] && printf 'cond <%s>\n' "${BASH_REMATCH[1]}"

echo "=== a command substitution is its own word and takes nothing with it"
p 'star in cmdsub'     "${n[@]} $(IFS=:; printf %s "${n[*]@Q}")"
