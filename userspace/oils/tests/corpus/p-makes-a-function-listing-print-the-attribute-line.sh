# `-p` alongside `-f`/`-F` is not a third listing flag but a switch that runs
# ahead of everything else: it turns the routes that would *mark* a function
# (`declare -fr g`, `-fx g`, `-ft g`) into listings that mark nothing, and it
# changes what a named listing says.
#
# `declare -F g` is the bare name; `declare -Fp g` is instead the attribute line
# the *nameless* `-F` listing prints — `declare -f g` — whether or not there is
# an attribute to report. `declare -f g` is the body; `declare -fp g` is the
# body followed by that same line, but only when the function carries an
# attribute, the body having already said everything a plain one has to say.
#
# And a name that is no function is silent without the `-p` and reported with
# it, under the name the command was written by. Each operand is answered where
# it is reached, so the complaints fall between the listings.
#
# extdebug decorates the bare name alone: `-F g` gains a line and a source,
# `-Fp g` has nowhere to put them.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

g() { :; }
h() { echo h; }
declare -ft h
export -f h

echo "== 1. -F, bare name against attribute line"
{ declare -F g h; echo "  rc=$?"; } 2>&1 | e
{ declare -Fp g h; echo "  rc=$?"; } 2>&1 | e

echo "== 2. every spelling of the same request"
for cmd in 'declare -Fp g' 'declare -pF g' 'declare -F -p g' 'declare -p -F g' \
           'declare -F +p g' 'declare -Fpg g' 'declare -Ffp g' 'declare -Fp -- g' \
           'typeset -Fp g'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 3. -f, body against body plus the line"
{ declare -f g; echo "  rc=$?"; } 2>&1 | e
{ declare -fp g; echo "  rc=$?"; } 2>&1 | e
{ declare -f h; echo "  rc=$?"; } 2>&1 | e
{ declare -fp h; echo "  rc=$?"; } 2>&1 | e

echo "== 4. a name that is no function"
for cmd in 'declare -F nope' 'declare -f nope' 'declare -Fp nope' \
           'declare -fp nope' 'typeset -Fp nope' 'typeset -fp nope'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done
v=1
{ declare -Fp v; echo "  declare -Fp v rc=$?"; } 2>&1 | e

echo "== 5. each operand is answered where it is reached"
{ declare -Fp nope g nada h; echo "  rc=$?"; } 2>&1 | e
{ declare -fp nope g nada; echo "  rc=$?"; } 2>&1 | e

echo "== 6. -p runs before any attribute would be applied"
for cmd in 'declare -frp g' 'declare -fxp g' 'declare -ftp g' 'declare -Frp g' \
           'declare -Ftp g' 'declare -Fxp g'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; declare -Fp g; } 2>&1 | e
done

echo "== 7. without the p they mark, as ever"
{ declare -Ft g; declare -Fp g; } 2>&1 | e
{ declare -Fx g; declare -Fp g; } 2>&1 | e

echo "== 8. extdebug decorates the bare name alone"
shopt -s extdebug
{ declare -F g; echo "  rc=$?"; } 2>&1 | e | sed -e 's|^\(    g\) [0-9][0-9]* .*$|\1 LINE FILE|'
{ declare -Fp g; echo "  rc=$?"; } 2>&1 | e
shopt -u extdebug

echo "== 9. the nameless listings are the same either way"
{ declare -F; } 2>&1 | e
{ declare -Fp; } 2>&1 | e
