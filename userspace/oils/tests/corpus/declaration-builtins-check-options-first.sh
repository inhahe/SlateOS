# The declaration builtins run one option scan over the whole leading flag
# cluster before anything else — before the routing that decides whether the
# command is a declaration, a `-p` listing, a `-f`/`-F` function listing or a
# `-ft` trace toggle. So an unknown letter is refused whatever the rest of the
# command would have done, and refused as the usage error it is: status 2, a
# `TAG: -X: invalid option` line, and the builtin's synopsis.
#
# The letter is reported under the sign it was written with, and punctuation
# counts as a letter — `declare -x=1` is not an assignment, it is the options
# `x` and `=`, so it is `-=` that is named. A `--` ends the scan, and what
# follows is then a bad *name* rather than a bad option.
#
# `local` has one refusal that comes even earlier: outside a function there is
# nothing to be local to, and it says so before reading a flag at all.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

echo "== 1. every route refuses the letter"
for cmd in 'declare -rq' 'declare -pq' 'declare -pq v' 'declare -Fq' \
           'declare -fq' 'declare -Fq q' 'declare -ftq q' 'declare -aq' \
           'declare -r -q' 'declare -rq --' 'declare -iq v=1'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 2. the sign it came under is the sign reported"
for cmd in 'declare +rq' 'declare +q' 'declare -q +z'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 3. punctuation is a letter too"
for cmd in 'declare -x=1' 'declare -=x' 'declare -=1' 'declare -rq=1' 'declare -a[0]'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 4. -- ends the scan, so what follows is a name"
{ declare -- -q; echo "  rc=$?"; } 2>&1 | e
{ declare -r -- -q; echo "  rc=$?"; } 2>&1 | e
f() { local -- -q; echo "  rc=$?"; }
f 2>&1 | e

echo "== 5. typeset and local speak under their own names"
{ typeset -rq; echo "  rc=$?"; } 2>&1 | e
{ typeset -pq; echo "  rc=$?"; } 2>&1 | e
g() { local -pq; echo "  rc=$?"; }
g 2>&1 | e
h() { local -rq; echo "  rc=$?"; }
h 2>&1 | e
k() { local -pq v; echo "  rc=$?"; }
k 2>&1 | e
m() { local -=1; echo "  rc=$?"; }
m 2>&1 | e

echo "== 6. local's older refusal comes first"
{ local -pq; echo "  rc=$?"; } 2>&1 | e
{ local -q; echo "  rc=$?"; } 2>&1 | e
{ local -- -q; echo "  rc=$?"; } 2>&1 | e

echo "== 7. the real letters are still taken"
{ declare -ax q=(1); declare -p q; echo "  rc=$?"; } 2>&1 | e
{ declare -ir n=3+4; declare -p n; echo "  rc=$?"; } 2>&1 | e
p() { local -p; echo "  rc=$?"; }
p 2>&1 | e
q() { local -a w=(z); declare -p w; }
q 2>&1 | e

echo "== 8. nothing was declared by any refusal"
declare -p v n 2>&1 | e
