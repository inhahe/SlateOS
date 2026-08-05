# A `[[ ]]` operand or a `case` word that met a **quoted** `[@]` is built out
# of words as well — the same path
# `a-cond-word-that-met-an-unquoted-at-list-is-built-out-of-words` describes,
# reached from the other side of the quotes.
#
# What the quoting changes is only which parts are protected, not whether the
# word takes the path:
#
#   * the quoted list's elements are words and keep every character they hold;
#   * the text around them — literals, scalars, command substitutions, and any
#     *unquoted* list in the same word — is still text that splits;
#   * the fields come back glued with single spaces, as always.
#
# So `IFS=:; [[ P:Q"${n[@]^^}" ]]` is `P QA:B C:D`: the literal split, the
# elements did not.
#
# The gate is *not* the unquoted spelling's. That one wants `$IFS`'s first
# character not to be a space; this one asks only that `$IFS` have something in
# it, so `IFS=' :'` and `IFS=$'\t'` are both on the path here and neither is
# there. A word holding both spellings shows the two gates at once: under
# `IFS=' :'`, `P:Q"${n[@]}"${n[@]}` keeps the quoted elements whole and splits
# the unquoted ones.
#
# The `:-`/`:+` family is the one shape whose answer is not knowable before it
# runs, and it is a list only on the branch that answers for the *operator*:
# `${e[@]:-A:B}` on an empty `e` answered with its operand and is no list,
# while `${e[@]:+A:B}` on the same `e` answered for itself — with nothing — and
# is. That is true of the unquoted spelling too.
#
# `[*]` never takes the path, and neither does a `${x:-…}` whose operand was
# not the branch taken.
#
# One context reads the finished list differently. A `[[ ]]` operand, a `case`
# subject and a `=~` right-hand side all glue the words back with spaces; a
# `case` **arm's pattern** keeps the *first word alone* and drops the rest, so
# `IFS=:; case X in P:Q"${n[@]^^}")` matches `P` and nothing longer. That is
# only true of the quoted path — the unquoted spelling stays one word, and its
# `case` pattern is the whole of it.

n=(a:b c:d)
e=()
o=('q:r')
declare -A m=([k:1]=v:1 [k:2]=v:2)
ref=n
SAVE=$IFS

r() { printf '  %-16s [%s]\n' "$1" "$2"; }

for lbl in colon colon-space tab-colon space-colon space tab null; do
  case $lbl in
    colon) ifs=':' ;;
    colon-space) ifs=': ' ;;
    tab-colon) ifs=$'\t:' ;;
    space-colon) ifs=' :' ;;
    space) ifs=' ' ;;
    tab) ifs=$'\t' ;;
    null) ifs='' ;;
  esac
  echo "=== IFS $lbl"

  # The literal beside a quoted list splits; the elements do not.
  ( IFS=$ifs; [[ P:Q"${n[@]^^}" =~ ^(.*)$ ]]; r 'lit+qderived' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${n[@]}"   =~ ^(.*)$ ]]; r 'lit+qplain'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ "${n[@]^^}"P:Q =~ ^(.*)$ ]]; r 'qderived+lit' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ "${n[@]^^}"    =~ ^(.*)$ ]]; r 'qderived'     "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ "${n[@]}"      =~ ^(.*)$ ]]; r 'qplain'       "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ "P:Q${n[@]^^}" =~ ^(.*)$ ]]; r 'all-quoted'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${o[@]^^}" =~ ^(.*)$ ]]; r 'one-element'  "${BASH_REMATCH[1]}" )

  # An empty list still counts: the expansion happened.
  ( IFS=$ifs; [[ P:Q"${e[@]}"   =~ ^(.*)$ ]]; r 'lit+qempty'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; set --; [[ P:Q"$@" =~ ^(.*)$ ]]; r 'qpos-empty'  "${BASH_REMATCH[1]}" )

  # Every other `[@]` spelling reaches the path too.
  ( IFS=$ifs; set -- a:b c:d; [[ P:Q"$@" =~ ^(.*)$ ]]; r 'qpositional' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${n[@]:0}"  =~ ^(.*)$ ]]; r 'q-slice'      "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${n[@]@Q}"  =~ ^(.*)$ ]]; r 'q-transform'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${!n[@]}"   =~ ^(.*)$ ]]; r 'q-keys'       "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${!m[@]}"   =~ ^(.*)$ ]]; r 'q-assoc-keys' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${!n@}"     =~ ^(.*)$ ]]; r 'q-varnames'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${!ref[@]}" =~ ^(.*)$ ]]; r 'q-indir-at'   "${BASH_REMATCH[1]}" )

  # …and the text around them is ordinary text, split as such. A scalar can
  # carry a separator that no literal could be written with.
  ( IFS=$ifs; v='y:z';   [[ $v"${n[@]^^}" =~ ^(.*)$ ]]; r 'col-scalar+q' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; v='y z';   [[ $v"${n[@]^^}" =~ ^(.*)$ ]]; r 'sp-scalar+q'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; v=$'y\tz'; [[ $v"${n[@]^^}" =~ ^(.*)$ ]]; r 'tab-scalar+q' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; v=$'y \tz';[[ $v"${n[@]^^}" =~ ^(.*)$ ]]; r 'sp-tab-sc+q'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P::Q"${n[@]^^}" =~ ^(.*)$ ]]; r 'lit-2sep+q'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ :P"${n[@]^^}"   =~ ^(.*)$ ]]; r 'lead-sep+q'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ "${n[@]^^}"P:   =~ ^(.*)$ ]]; r 'trail-sep+q'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"$(echo m:n)${n[@]^^}" =~ ^(.*)$ ]]; r 'q-sub' "${BASH_REMATCH[1]}" )

  # Both spellings in one word: the quoted elements are protected under every
  # `$IFS`, the unquoted ones only where the unquoted gate is on.
  ( IFS=$ifs; [[ P:Q"${n[@]}"${n[@]}     =~ ^(.*)$ ]]; r 'qplain+unq'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q${n[@]^^}"${n[@]^^}" =~ ^(.*)$ ]]; r 'unq+q'       "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${n[@]^^}"${n[@]^^} =~ ^(.*)$ ]]; r 'q+unq'       "${BASH_REMATCH[1]}" )

  # A quoted `[*]`, a length, a scalar indirection and a command substitution
  # are no list at all.
  ( IFS=$ifs; [[ P:Q"${n[*]}"   =~ ^(.*)$ ]]; r 'qstar-plain' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${n[*]^^}" =~ ^(.*)$ ]]; r 'qstar-deriv' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${#n[@]}"  =~ ^(.*)$ ]]; r 'q-length'    "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${!ref}"   =~ ^(.*)$ ]]; r 'q-indirect'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"$(echo "${n[@]}")" =~ ^(.*)$ ]]; r 'q-cmdsub' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; v='y:z'; [[ P:Q"$v" =~ ^(.*)$ ]]; r 'qscalar'   "${BASH_REMATCH[1]}" )

  # A `${x:-…}` reaches the path from either side of the quotes, and only on
  # the branch it actually took.
  ( IFS=$ifs; x=;    [[ P:Q"${x:-${n[@]^^}}" =~ ^(.*)$ ]]; r 'qop-taken'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=set; [[ P:Q"${x:-${n[@]^^}}" =~ ^(.*)$ ]]; r 'qop-not'    "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=set; [[ P:Q"${x:+${n[@]^^}}" =~ ^(.*)$ ]]; r 'qalt-taken' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=;    [[ P:Q"${x:+${n[@]^^}}" =~ ^(.*)$ ]]; r 'qalt-not'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; x=;    [[ P:Q${x:-"${n[@]^^}"} =~ ^(.*)$ ]]; r 'unq-op-q'   "${BASH_REMATCH[1]}" )

  # The `[@]:-` family: a list only where the operator answered for itself.
  ( IFS=$ifs; [[ P:Q"${n[@]:-w}"   =~ ^(.*)$ ]]; r 'qop-array'    "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${e[@]:-A:B}" =~ ^(.*)$ ]]; r 'qop-empty'    "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${e[@]:+A:B}" =~ ^(.*)$ ]]; r 'qalt-empty'   "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${n[@]:+A:B}" =~ ^(.*)$ ]]; r 'qalt-full'    "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q"${e[*]:-A:B}" =~ ^(.*)$ ]]; r 'qop-empty-*'  "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q${n[@]:-w}     =~ ^(.*)$ ]]; r 'unq-op-array' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q${e[@]:-A:B}   =~ ^(.*)$ ]]; r 'unq-op-empty' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q${e[@]:+A:B}   =~ ^(.*)$ ]]; r 'unq-alt-empty' "${BASH_REMATCH[1]}" )
  ( IFS=$ifs; [[ P:Q${n[@]:+A:B}   =~ ^(.*)$ ]]; r 'unq-alt-full' "${BASH_REMATCH[1]}" )

  # A `case` word, a `case` pattern and a `=~` right-hand side are cond words
  # too, and take the quoted path on the same terms.
  ( IFS=$ifs; case P:Q"${n[@]^^}" in 'P QA:B C:D') r 'case-subj' split;;
    'P:QA:B C:D') r 'case-subj' nosplit;; *) r 'case-subj' other;; esac )
  ( IFS=$ifs; for s in 'P QA:B C:D' 'P:QA:B C:D' 'P:QA:B' 'P'; do
      case "$s" in P:Q"${n[@]^^}") r 'case-pat' "$s";; esac; done )
  ( IFS=$ifs; for s in 'A:B C:DX Y' 'A:B C:DX' 'A:B'; do
      case "$s" in "${n[@]^^}"X:Y) r 'case-pat-tail' "$s";; esac; done )
  ( IFS=$ifs; for s in 'P QA:B' 'P:QA:B' 'P'; do
      case "$s" in P:Q"${o[@]^^}") r 'case-pat-one' "$s";; esac; done )
  ( IFS=$ifs; for s in 'P Q' 'P:Q' 'P'; do
      case "$s" in P:Q"${e[@]}") r 'case-pat-empty' "$s";; esac; done )
  ( IFS=$ifs; for s in 'P QA B C D' 'P:QA:B C:D' 'P'; do
      case "$s" in P:Q${n[@]^^}) r 'case-pat-unq' "$s";; esac; done )
  ( IFS=$ifs; for s in 'P' 'P QA B C DA:B C:D' 'P:QA:B C:DA:B C:D'; do
      case "$s" in P:Q${n[@]^^}"${n[@]^^}") r 'case-pat-both' "$s";; esac; done )
  ( IFS=$ifs; [[ 'P QA:B C:D' =~ ^P:Q"${n[@]^^}"$ ]] && r 'regex-rhs' split || r 'regex-rhs' other )
  ( IFS=$ifs; [[ 'P QA:B C:D' == P:Q"${n[@]^^}" ]] && r 'cond-pat' split || r 'cond-pat' other )
  ( IFS=$ifs; [[ 'P:QA:B C:D' == P:Q"${n[@]^^}" ]] && r 'cond-pat-ns' split || r 'cond-pat-ns' other )
done

IFS=$SAVE
echo done
