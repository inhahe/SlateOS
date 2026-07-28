# A sourced script fires the RETURN trap when it finishes — not gated on
# functrace, but gated on the caller's mask.
printf 'S="$S insrc"
' > inner.sh
trap 'S="$S ret"' RETURN
. ./inner.sh
S="$S after"
echo "[$S]"
