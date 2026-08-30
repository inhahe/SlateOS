# A declaration is not an assignment. `declare -a q` leaves the name existing
# but *invisible*: `declare -p` reports it — that is what the state is for —
# while every listing built from values passes over it. `${!q*}`, `${!q@}`,
# `compgen -A variable` and `compgen -A arrayvar` are all that same listing, so
# they agree with each other and disagree with `declare -p`.
#
# Anything that puts a value there makes the name visible, including the empty
# `q=()`; nothing else does — an attribute, an export, a `local` in a frame
# nobody is looking at. `unset` takes the name back out of the listing whether
# it was visible or not.
#
# The shell's own variables sit on both sides of this. The ones it computes are
# listed as the scalars they are; the call-stack arrays are listed too, but
# only once each — being both a table entry and a computed one is not two
# variables.

p() { echo "$1 | star=[${!zz*}] at=[${!zz@}] var=[$(compgen -A variable zz | tr '\n' ' ')]"; }

echo "=== a declaration is not an assignment"
p 'nothing yet'
declare -a zz1;              p 'declare -a zz1'
declare -A zz2;              p 'declare -A zz2'
declare -i zz3;              p 'declare -i zz3'
declare -r zz4;              p 'declare -r zz4'
export zz5;                  p 'export zz5'
local zz6 2>/dev/null;       p 'local zz6 (at top level)'
declare -p zz1 zz2 zz3

echo "=== …but a value is"
zz1=();                      p 'zz1=()'
zz2=();                      p 'zz2=()'
zz3=0;                       p 'zz3=0'
declare -a zz7; zz7[0]=x;    p 'zz7[0]=x'
declare -a zz8; zz8+=(x);    p 'zz8+=(x)'
declare -A zz9; zz9[k]=v;    p 'zz9[k]=v'
declare -a zza=();           p 'declare -a zza=()'
declare zzb=;                p 'declare zzb='

echo "=== and unset takes it back out"
unset zz1;                   p 'unset zz1'
unset zz7;                   p 'unset zz7'
unset zz5;                   p 'unset zz5 (never visible)'

echo "=== a frame nobody is looking at does not leak"
f() { local -a zzc; local zzd=1; echo "in-fn [${!zz*}]"; }
f
p 'after f'

echo "=== arrayvar is the array-valued subset"
echo "arrayvar zz: [$(compgen -A arrayvar zz | tr '\n' ' ')]"
echo "arrayvar zz3: [$(compgen -A arrayvar zz3)]"

echo "=== the shell's own names are listed once each"
for n in BASH_SOURCE BASH_LINENO BASH_VERSINFO PIPESTATUS SECONDS RANDOM LINENO PPID BASHPID EPOCHSECONDS; do
  echo "$n: var=[$(compgen -A variable "$n" | tr '\n' ' ')] arrayvar=[$(compgen -A arrayvar "$n" | tr '\n' ' ')]"
done
