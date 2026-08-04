# `"${a[@]@A}"` is not one word. The declaration the transform builds is
# field-split against `$IFS` *inside the double quotes*, where an ordinary
# expansion never is, so the default `IFS` makes it the three words `declare`,
# `-a` and `n=([0]="x" [1]="y")` — and the space inside that third word shows
# the split is not a re-split of the finished string on spaces alone.
#
# It really is a raw character split, not a re-tokenisation: `IFS=a` cuts
# `declare` in half. What it respects is the regions a shell would read as one
# word — `'…'`, `"…"` and `(…)` — so the delimiters *inside* the element list
# never cut, and a `)` in a value does not end the group.
#
# An empty `IFS` does not mean "no delimiter" here, the way it does for a join:
# bash marks the word "split on space anyway", so `IFS=` gives the same three
# words that `IFS=' '` does. Only the `[*]` spelling joins, and it joins the
# items it had *before* any splitting — which is why `${*@A}` under `IFS=`
# comes back as `set -- 'p q''r'` with nothing between the parameters.
#
# The scalar forms are not collections and are not split: `${iv[@]@A}` stays
# the single word `declare -i iv='7'` even under the default `IFS`.

declare -a n=(x y)
declare -A m=([k]=v)
declare -i iv=7
s='a b'

show() { printf '  %-8s count=%s' "$1" "$(($# - 1))"; shift; printf ' [%s]' "$@"; echo; }

echo "=== the declaration is split, and the scalar forms are not"
for i in ' ' ':' '' 'a' '=' 'x' 'e'; do
  echo "--- IFS=[$i]"
  ( IFS=$i; set -- "${n[@]@A}";  show 'arr@'  "$@" )
  ( IFS=$i; set -- "${n[*]@A}";  show 'arr*'  "$@" )
  ( IFS=$i; set -- "${m[@]@A}";  show 'assoc' "$@" )
  ( IFS=$i; set -- "${iv[@]@A}"; show 'scal'  "$@" )
  ( IFS=$i; set -- "${s[@]@A}";  show 'plain' "$@" )
done

echo "=== the positional form is one item per parameter, head glued to the first"
set -- 'p q' r
for i in ' ' ':' '' 'a' '='; do
  ( IFS=$i; set -- "${@@A}"; show "@[$i]" "$@" )
done
set -- 'p q' r
for i in ' ' ':' '' 'a' '='; do
  ( IFS=$i; set -- "${*@A}"; show "*[$i]" "$@" )
done

echo "=== a scalar context joins the items and never splits them"
# The split belongs to the `[@]` *field* context, so an assignment — which wants
# one string — gets the items back whole and glues them with the separator the
# spelling asks for. The declaration therefore survives an `IFS` that would have
# cut it to pieces, and the positional form shows which separator each spelling
# used, since it is the one with more than a single item.
for i in ' ' ':' '' 'a'; do
  echo "--- IFS=[$i]"
  ( IFS=$i; x="${n[@]@A}"; printf '  arr@   [%s]\n' "$x" )
  ( IFS=$i; x="${n[*]@A}"; printf '  arr*   [%s]\n' "$x" )
  ( IFS=$i; set -- 'p q' r; x="${@@A}"; printf '  pos@   [%s]\n' "$x" )
  ( IFS=$i; set -- 'p q' r; x="${*@A}"; printf '  pos*   [%s]\n' "$x" )
done

echo "=== an unquoted reference splits the joined string, on top of any of this"
# Unquoted, the items are joined first and the ordinary field split runs over
# the result — which is why the default `IFS` cuts inside the element list here
# and not in the quoted form: `n=([0]="x"` and `[1]="y")` are two words.
for i in ' ' ':' 'a'; do
  ( IFS=$i; set -- ${n[@]@A}; show "u[$i]" "$@" )
done

echo "=== an unset IFS splits like the default one"
( unset IFS; set -- "${n[@]@A}"; show 'unset' "$@" )
( unset IFS; set -- "${n[*]@A}"; show 'unset*' "$@" )

echo "=== the quoted and parenthesised regions are passed over"
declare -a p=('a)b' 'c d')
declare -a q=("i'j" 'k"l')
( IFS=' '; set -- "${p[@]@A}"; show 'paren' "$@" )
( IFS=' '; set -- "${q[@]@A}"; show 'quote' "$@" )
declare -a r=(' lead' 'trail ')
( IFS=' '; set -- "${r[@]@A}"; show 'edge' "$@" )
( IFS='e'; set -- "${r[@]@A}"; show 'edge-e' "$@" )

echo "=== a declaration with no elements, and a name with nothing to declare"
declare -a du
declare -a em=()
declare -arx w=(1)
( IFS=' '; set -- "${du[@]@A}"; show 'bare'  "$@" )
( IFS=' '; set -- "${em[@]@A}"; show 'empty' "$@" )
( IFS=' '; set -- "${w[@]@A}";  show 'flags' "$@" )
( IFS=' '; set -- "${nope[@]@A}"; show 'nope' "$@" )

echo "=== the letters form is one word per element and never splits"
( IFS=' '; set -- "${n[@]@a}"; show 'a-arr' "$@" )
( IFS=' '; set -- "${w[@]@a}"; show 'a-flg' "$@" )
set -- p q
( IFS=' '; set -- "${@@a}"; show 'a-pos' "$@" )
