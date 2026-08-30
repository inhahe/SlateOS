# A `[[ ]]` operand or a `case` word that met an **unquoted** `[@]` anywhere in
# it is not built as text at all: bash builds it as a list of *words* and glues
# them back with single spaces. The list is the `WORD_LIST` path
# `a-default-words-nested-list-is-not-split` describes for the `${x:-w}`
# operand, reached from a second context.
#
# What that changes is the whole word, not the expansion that triggered it:
#
#   * the elements of a *plain* `[@]` (`${n[@]}`, `$@`, and `${n[@]:-w}` on the
#     branch where it answers with the array) are words, so their own `$IFS`
#     characters do not split;
#   * everything else contributes text whose characters do split — literals,
#     scalars, command substitutions, and the elements of a *derived* `[@]`
#     (a slice, `^^`, `#p`, `@Q`, `/a/Z`, a key list), which are first glued
#     with `$IFS`'s own first character;
#   * a quoted stretch is protected, as always.
#
# So `IFS=:; [[ P:Q${n[@]^^} ]]` is `P QA B C D`: the literal `P:Q` split too,
# because the `[@]` beside it moved the word onto the path.
#
# The gate is `$IFS` non-null AND its *first* character not a space. Only a
# space is special, not whitespace in general — `$'\t:'` is on the path and
# `' :'` is not — and that is a narrower question than the operand's, which
# wants no space anywhere (`': '` is on the path here and joins nothing there).
#
# The `[*]` spellings never take the path, and neither does anything inside
# double quotes.

n=(a:b c:d)
e=()
o=('q:r')
z=(':' 'x')
declare -A m=([k:1]=v:1 [k:2]=v:2)
SAVE=$IFS

# The value of a cond word, read back without a splitting context of its own.
r() { printf '  %-14s [%s]\n' "$1" "$2"; }

for lbl in colon colon-space tab-colon space-colon space null trailing; do
  case $lbl in
    colon) ifs=':' ;;
    colon-space) ifs=': ' ;;
    tab-colon) ifs=$'\t:' ;;
    space-colon) ifs=' :' ;;
    space) ifs=' ' ;;
    null) ifs='' ;;
    trailing) ifs=':x' ;;
  esac
  echo "=== IFS $lbl"

  # A plain `[@]`: the elements are words and keep their own separators; only
  # the break between them becomes a space.
  ( IFS=$ifs; [[ ${n[@]} =~ ^(.*)$ ]]; r 'plain' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; set -- a:b c:d; [[ $@ =~ ^(.*)$ ]]; r 'positional' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]:-w} =~ ^(.*)$ ]]; r 'plain-default' "${BASH_REMATCH[1]}" )

  # A derived one: glued with `$IFS` and then split, so every separator the
  # elements held comes back as a space too.
  ( IFS=$ifs; [[ ${n[@]^^} =~ ^(.*)$ ]]; r 'case^^' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]#a} =~ ^(.*)$ ]]; r 'trim#' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]/a/Z} =~ ^(.*)$ ]]; r 'replace' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]@Q} =~ ^(.*)$ ]]; r 'transform@Q' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]:0} =~ ^(.*)$ ]]; r 'slice' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]:0:1} =~ ^(.*)$ ]]; r 'slice-1' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${m[@]^^} =~ ^(.*)$ ]]; r 'assoc^^' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${!m[@]} =~ ^(.*)$ ]]; r 'assoc-keys' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${o[@]^^} =~ ^(.*)$ ]]; r 'one-element' "${BASH_REMATCH[1]}" )

  # An *empty* array still counts as having met the list: the literal beside it
  # splits even though the expansion produced nothing.
  ( IFS=$ifs; [[ P:Q${e[@]^^} =~ ^(.*)$ ]]; r 'empty-list' "${BASH_REMATCH[1]}" )

  # The word around the expansion is on the path too.
  ( IFS=$ifs; [[ P:Q${n[@]^^} =~ ^(.*)$ ]]; r 'lit+derived' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]^^}P:Q =~ ^(.*)$ ]]; r 'derived+lit' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P${n[@]^^}S =~ ^(.*)$ ]]; r 'lit-around' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q${n[@]} =~ ^(.*)$ ]]; r 'lit+plain' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; v='y:z'; [[ $v${n[@]^^} =~ ^(.*)$ ]]; r 'scalar+derived' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ $(echo m:n)${n[@]^^} =~ ^(.*)$ ]]; r 'sub+derived' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]}${n[@]^^} =~ ^(.*)$ ]]; r 'plain+derived' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[@]^^}${n[@]} =~ ^(.*)$ ]]; r 'derived+plain' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ 'P:Q'${n[@]^^} =~ ^(.*)$ ]]; r 'sqlit+derived' "${BASH_REMATCH[1]}" )

  # A literal with no list beside it is text, and a scalar is text: neither is
  # on the path by itself.
  ( IFS=$ifs; [[ P:Q =~ ^(.*)$ ]]; r 'lit-alone' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; v='y:z'; [[ P:Q$v =~ ^(.*)$ ]]; r 'lit+scalar' "${BASH_REMATCH[1]}" )

  # The glue is `$IFS`'s first character rather than a space already, and that
  # is visible where the separator is `$IFS` whitespace: a whitespace glue
  # absorbs the empty field beside it, a `:` glue does not.
  ( IFS=$ifs; [[ ${z[@]^^} =~ ^(.*)$ ]]; r 'sep-only' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; y=('' 'x'); [[ ${y[@]^^} =~ ^(.*)$ ]]; r 'empty-el' "${BASH_REMATCH[1]}" )

  # The `[*]` spellings keep `$IFS` and are never rebuilt out of words.
  ( IFS=$ifs; [[ ${n[*]^^} =~ ^(.*)$ ]]; r 'star^^' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q${n[*]^^} =~ ^(.*)$ ]]; r 'lit+star' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ ${n[*]} =~ ^(.*)$ ]]; r 'star-plain' "${BASH_REMATCH[1]}" )

  # Inside double quotes the path is off for the expansion's own join.
  ( IFS=$ifs; [[ "${n[@]^^}" =~ ^(.*)$ ]]; r 'quoted' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ "${n[@]}" =~ ^(.*)$ ]]; r 'quoted-plain' "${BASH_REMATCH[1]}" )

  # A `${x:-w}` operand reaches the path through its own word list: the list is
  # the operand's, the splitting is the cond word's.
  ( IFS=$ifs; x=; [[ ${x:-${n[@]^^}} =~ ^(.*)$ ]]; r 'op-derived' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=; [[ ${x:-${n[@]:0}} =~ ^(.*)$ ]]; r 'op-slice' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=; [[ ${x:-${n[@]}} =~ ^(.*)$ ]]; r 'op-plain' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=; [[ ${x:-${n[@]}:Z} =~ ^(.*)$ ]]; r 'op-plain-lit' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=; [[ ${x:-P:Q${n[@]^^}} =~ ^(.*)$ ]]; r 'op-lit+derived' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=; [[ ${x:-P:Q} =~ ^(.*)$ ]]; r 'op-lit-alone' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=; [[ P:Q${x:-${n[@]^^}} =~ ^(.*)$ ]]; r 'lit+op' "${BASH_REMATCH[1]}" )

  # A `=~` right-hand side is a cond word too, so the pattern it builds is
  # rebuilt out of words in exactly the same way — the separators the elements
  # held reach the regex as spaces.
  ( IFS=$ifs; [[ 'A B C D' =~ ^${n[@]^^}$ ]] && r '=~ derived' split || r '=~ derived' nosplit )
  ( IFS=$ifs; [[ 'A:B C:D' =~ ^${n[@]^^}$ ]] && r '=~ derived-c' yes || r '=~ derived-c' no )
  ( IFS=$ifs; [[ 'a:b c:d' =~ ^${n[@]}$ ]] && r '=~ plain' yes || r '=~ plain' no )
  ( IFS=$ifs; [[ 'P Qa:b c:d' =~ ^P:Q${n[@]}$ ]] && r '=~ lit+plain' split || r '=~ lit+plain' nosplit )
  ( IFS=$ifs; [[ 'P QA B C D' =~ ^P:Q${n[@]^^}$ ]] && r '=~ lit+der' split || r '=~ lit+der' nosplit )
  ( IFS=$ifs; [[ 'P:QA B C D' =~ ^"P:Q"${n[@]^^}$ ]] && r '=~ qlit+der' kept || r '=~ qlit+der' other )
  ( IFS=$ifs; [[ 'A:B:C:D' =~ ^${n[*]^^}$ ]] && r '=~ star' ifs || r '=~ star' other )
  ( IFS=$ifs; v='y:z'; [[ 'y zA B C D' =~ ^$v${n[@]^^}$ ]] && r '=~ scalar' split || r '=~ scalar' nosplit )

  # The same word as a `case` subject, as a `case` pattern, and as the right
  # operand of `[[ == ]]` — all three are cond words.
  ( IFS=$ifs; case P:Q${n[@]^^} in 'P QA B C D') r 'case-subj' split;;
    'P:QA:B C:D') r 'case-subj' nosplit;; *) r 'case-subj' other;; esac )
  ( IFS=$ifs; case 'P QA B C D' in P:Q${n[@]^^}) r 'case-pat' split;;
    *) r 'case-pat' other;; esac )
  ( IFS=$ifs; [[ 'P QA B C D' == P:Q${n[@]^^} ]] && r 'cond-pat' split || r 'cond-pat' other )
done

IFS=$SAVE
echo done
