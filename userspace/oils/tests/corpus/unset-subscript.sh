# `unset name[sub]` is the one place where a subscript is written as an ordinary
# *word* — the shell has already expanded the argument once, and `unset`
# re-parses what is left. That double life is where the bugs are: the subscript
# has to be arithmetic for an indexed array, a string key for an associative
# one, and a few shapes are not array references at all.
#
# Iteration order for associative arrays is hash-dependent, so anything
# order-sensitive is sorted before printing.

sorted() { printf '%s\n' "$@" | sort | tr '\n' ' '; echo; }

echo "=== indexed subscripts are arithmetic ==="
a=(0 1 2 3 4)
i=2
unset 'a[1+1]'; echo "expr st=$? keys=${!a[*]}"
unset 'a[i]'; echo "var st=$? keys=${!a[*]}"
unset 'a[$(echo 3)]'; echo "cmdsub st=$? keys=${!a[*]}"
# Removing an element leaves a *gap*: higher elements keep their indices.
b=(a b c d)
unset 'b[1]'; echo "gap keys=${!b[*]} vals=${b[*]} n=${#b[@]}"

echo "=== negative indices count back from the highest index + 1 ==="
c=(0 1 2 3 4)
unset 'c[-1]'; echo "last st=$? keys=${!c[*]}"
unset 'c[-3]'; echo "third st=$? keys=${!c[*]}"
# A *sparse* array counts from its highest index, not its element count.
declare -a sp=([0]=a [5]=b)
unset 'sp[-1]'; echo "sparse st=$? keys=${!sp[*]}"
# Underflowing past 0 names the subscript's *source* in brackets, with no
# variable name — and does not stop the remaining names on the same command.
d=(0 1 2); e=(0 1 2); f=1
unset 'd[-9]' 'e[0]' f; echo "under st=$? d=${#d[@]} e=${!e[*]} f=${f-gone}"
# An in-range index that simply has no element is a silent no-op.
unset 'e[99]'; echo "oor st=$? e=${!e[*]}"

echo "=== [@] and [*] on an indexed array empty it but keep it declared ==="
declare -ai g=(1 2 3)
unset 'g[@]'; echo "at st=$? n=${#g[@]}"
declare -p g
declare -a h=(1 2 3)
unset 'h[*]'; echo "star st=$? n=${#h[@]}"
declare -p h

echo "=== associative subscripts are string keys ==="
declare -A m=([alpha]=1 [beta]=2 ['two words']=3)
unset 'm[beta]'; echo "plain st=$? n=${#m[@]}"
sorted "${!m[@]}"
unset 'm[two words]'; echo "spaces st=$? n=${#m[@]}"
# Quotes inside the subscript are removed, and expansions run.
declare -A q=([k]=1 [v]=2 [w]=3)
key=v
unset "q['k']"; echo "sq st=$? n=${#q[@]}"
unset 'q[$key]'; echo "expand st=$? n=${#q[@]}"
sorted "${!q[@]}"
# …so `@` and `*` are ordinary keys here: they remove one entry, never all.
declare -A s=([a]=1 [@]=at [*]=star)
unset 's[@]'; echo "at st=$? n=${#s[@]}"
unset 's[*]'; echo "star st=$? n=${#s[@]}"
sorted "${!s[@]}"
# An all-elements subscript on a map with no such key removes nothing at all.
declare -A t=([a]=1 [b]=2)
unset 't[@]'; echo "noflush st=$? n=${#t[@]}"
# An empty key has no representation, so it is a bad subscript — again named
# by source, so the expansion that produced it stays visible.
declare -A z=([k]=v)
blank=
unset 'z[$blank]'; echo "empty st=$? n=${#z[@]}"

echo "=== a scalar is addressable only as [0] ==="
s1=hello
unset 's1[0]'; echo "zero st=$? exists=${s1+yes}"
s2=hello
unset 's2[1-1]'; echo "arith-zero st=$? exists=${s2+yes}"
s3=hello
unset 's3[1]'; echo "one st=$? exists=${s3+yes}"
s4=hello
unset 's4[-1]'; echo "neg st=$? exists=${s4+yes}"
s5=hello
unset 's5[@]'; echo "at st=$? exists=${s5+yes}"

echo "=== an unset variable is never even probed ==="
# No array exists, so bash does not evaluate the subscript: a malformed
# expression that would be an error on a real array is silently fine here.
unset 'nosuch[x y]'; echo "badarith st=$?"
unset 'nosuch[-5]'; echo "underflow st=$?"

echo "=== tokens that are not array references ==="
# An empty subscript, trailing junk after the `]`, and a base that is not an
# identifier are all ordinary (necessarily unset) variable names — no error.
n1=(1 2)
unset 'n1[]'; echo "emptysub st=$? n=${#n1[@]}"
n2=(1 2)
unset 'n2[0]junk'; echo "junk st=$? n=${#n2[@]}"
unset '1abc[0]'; echo "badname st=$?"

echo "=== namerefs resolve to the target array ==="
declare -a na=(1 2 3)
declare -n ref=na
unset 'ref[1]'; echo "nameref st=$? keys=${!na[*]}"

echo "=== readonly ==="
declare -A ro=([k]=v); readonly ro
other=1
unset 'ro[k]' other; echo "ro st=$? n=${#ro[@]} other=${other-gone}"

echo "=== a malformed arithmetic subscript discards the parse unit ==="
bad=(1 2 3)
unset 'bad[x y]'; echo "unreachable"
echo "after=$? n=${#bad[@]}"

echo done
