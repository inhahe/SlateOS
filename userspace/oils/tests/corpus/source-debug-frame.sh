# A sourced script IS a DEBUG frame: commands inside it are masked without -T.
printf 'X=1
' > inner.sh
trap 'D="$D|$BASH_COMMAND"' DEBUG
. ./inner.sh
trap - DEBUG
echo "untraced=[$D]"
D=
set -T
trap 'D="$D|$BASH_COMMAND"' DEBUG
. ./inner.sh
trap - DEBUG
echo "traced=[$D]"
