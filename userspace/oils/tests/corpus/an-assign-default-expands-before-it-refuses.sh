# `${a[@]:=w}` cannot do what it says — there is no `a[@]` to write to — but it
# expands `w` anyway, and only then complains. So a command substitution in the
# default word runs, and whatever it changed stays changed, even though the
# assignment never happens.
#
# The one form that refuses *without* expanding is the positional `${@:=w}`:
# the positionals are not a variable at all, so bash rejects the shape rather
# than the subscript, and it does so first. The two also differ in status — the
# subscript complaint discards with 2, the positional refusal with 1.
#
# An associative array is not a near-miss but a different answer: its subscript
# is a *string*, so `@` is a perfectly good key and `${m[@]:=w}` really assigns
# `m["@"]`.
#
# An active reference reads nothing at all, which is the ordinary `:-` rule and
# is what makes "expanded before it refuses" worth stating separately.

f() { printf 'ran:%s\n' "$1" >&2; printf 'V%s' "$1"; }

echo "### an active array never reads the default"
a=(x y)
echo "[${a[@]:=$(f active-at)}]"
echo "[${a[*]:=$(f active-star)}]"
echo "[${a[@]=$(f active-colonless)}]"

echo "### an inactive one reads it, then refuses"
e=()
echo "[${e[@]:=$(f empty-at)}]"
echo "  after: $?"

echo "### and so does a name that was never declared"
echo "[${undeclared[@]:=$(f undeclared-at)}]"
echo "  after: $?"

echo "### the star spelling and the colon-less one, the same way"
g=()
echo "[${g[*]:=$(f empty-star)}]"
echo "  after: $?"
h=()
echo "[${h[@]=$(f empty-colonless)}]"
echo "  after: $?"

echo "### the positionals refuse the shape, before reading anything"
set --
echo "[${@:=$(f positional)}]"
echo "  after: $?"
echo "[${*:=$(f positional-star)}]"
echo "  after: $?"

echo "### an associative array has a key called @, so it just assigns"
declare -A m
echo "[${m[@]:=$(f assoc-at)}]"
echo "  after: $?"
declare -p m
declare -A n
echo "[${n[*]:=$(f assoc-star)}]"
declare -p n

echo "### a side effect in the default word outlives the refusal"
p=()
echo "[${p[@]:=${z:=set-z}}]"
echo "  z=[$z] after=$?"

echo "### the scalar spelling assigns for real, as always"
unset s
echo "[${s:=$(f scalar)}]"
echo "  s=[$s] after=$?"

echo "### and an indirection follows the array it lands on"
q=()
r=q
echo "[${!r:=$(f indirect)}]"
echo "  after: $?"
echo "done"
