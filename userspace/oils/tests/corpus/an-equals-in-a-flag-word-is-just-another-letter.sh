# A word is an option word because of its leading `-`, not because of what
# follows it. getopt has no notion of an assignment, so an `=` inside a flag
# cluster is simply another letter — and an unknown one — which makes
# `export -x=1` a usage error rather than an attempt to bind a name spelled
# `-x`. The letter reported is always the first one the builtin does not take,
# so `-n=1` names `-=` (the `n` was fine) while `-x=1` names `-x`.
#
# The scan is still only over the *leading* words: once an operand has been
# seen, or a `--` written, a later `-x=1` is a name again and draws the
# identifier refusal instead.
#
# `readonly`, `declare`, `typeset` and `local` all read it the same way; the
# usage line each prints is its own.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

echo "== 1. export"
for cmd in 'export -x=1' 'export -q=1' 'export -=1' 'export -=x' \
           'export -n=1' 'export -f=1' 'export -p=1' 'export -na=1'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 2. readonly"
for cmd in 'readonly -=1' 'readonly -r=1' 'readonly -p=1' 'readonly -a=1' \
           'readonly -f=1'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 3. the declare family"
for cmd in 'declare -n=1' 'declare -g=1' 'declare -x=1' 'typeset -a=1'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done
f() { local -n=1; echo "  local -n=1 rc=$?"; }
f 2>&1 | e

echo "== 4. a plus word is not scanned by these two at all"
for cmd in 'export +x=1' 'export +n=1' 'readonly +x=1'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 5. only the leading words are scanned"
{ export a=1 -x=2; echo "  rc=$?"; declare -p a; } 2>&1 | e
{ export -- -x=1; echo "  rc=$?"; } 2>&1 | e
{ readonly -- -a=1; echo "  rc=$?"; } 2>&1 | e
{ declare -- -x=1; echo "  rc=$?"; } 2>&1 | e

echo "== 6. a lone dash is an operand, not an empty cluster"
{ export -; echo "  export - rc=$?"; } 2>&1 | e
{ readonly -; echo "  readonly - rc=$?"; } 2>&1 | e

echo "== 7. nothing was bound by any of it"
declare -p a 2>&1 | e
