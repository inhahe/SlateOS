# Arithmetic text reaches the evaluator by two different routes, and the routes
# disagree about who expands it.
#
# A `(( … ))` command and the sections of a `for (( … ))` are text the *parser*
# set aside raw — nothing has expanded them yet, so the shell expands `$params`
# in them on the way to the evaluator. A `let` argument, and the value of an
# integer-attributed assignment, arrive as ordinary words that word expansion
# has already finished with; the shell expands them no further. So a `$` that
# survived quoting reaches the evaluator, which has no rule for one.
#
# The consequence is a syntax error where a reader might expect a value, and it
# is the *quoting* that decides which route the text took.

echo "=== let does not expand its argument ==="
n=5
let 'x=$n+1'
echo "let rc=$? x=[$x]"
let 'y=${n}+1'
echo "braced rc=$? y=[$y]"
# Even a nested `$(( … ))` is opaque here — `let` never runs an expansion pass.
let 'z=$((n))'
echo "nested rc=$? z=[$z]"
# `$?` and `$#` are no different: it is the `$` that is unparseable, not the name.
let 'q=$?+1'
echo "status rc=$? q=[$q]"

echo "=== but the same text unquoted is expanded before let ever sees it ==="
# Here the *shell* expanded `$n` while building the word, so `let` gets `x=5+1`.
let "x=$n+1"
echo "unquoted rc=$? x=[$x]"
let x=$n+1
echo "bare rc=$? x=[$x]"

echo "=== an integer assignment is the same, and it is fatal ==="
# Only the value is echoed (the name is not part of the arithmetic), the
# `declare` tag is applied, and the rest of the line does not run.
declare -i k='$n+1'; echo "unreachable-declare"
echo "declare rc=$? k=[$k]"
# Written without the builtin, the same refusal loses the tag with it.
declare -i j
j='$n+1'; echo "unreachable-assign"
echo "assign rc=$? j=[$j]"
# An element of an integer array is no different.
declare -ia ia
ia[0]='$n+1'; echo "unreachable-elem"
echo "elem rc=$? ia0=[${ia[0]}]"

echo "=== the contrast: (( )) and for (( )) do expand ==="
(( x=$n+1 ))
echo "arith-cmd rc=$? x=[$x]"
# Quoting cannot hide the `$` from that pass: `((` is handed the section text
# with its quotes still in it, and expansion runs over the whole thing.
for ((i=$n; i<7; i++)); do echo "for i=$i"; done
echo "for rc=$?"

echo "=== a nested \$(( )) failure in a section is an expansion error ==="
# It is therefore untagged and fatal, unlike the `((`-tagged, merely-failing
# error that bad arithmetic in the same place would give. Each probe is alone on
# its line because the abort abandons the rest of the parse unit.
(( $((1/0)) ))
echo "arith-nested rc=$?"
for ((i=$((1/0)); i<1; i++)); do echo "unreachable-init"; done
echo "for-init rc=$?"
for ((i=0; i<$((1/0)); i++)); do echo "unreachable-cond"; done
echo "for-cond rc=$?"
for ((i=0; i<1; i=$((1/0)))); do echo "for-body-once"; done
echo "for-update rc=$?"
# The control: bad arithmetic that is not a nested expansion keeps the tag and
# only fails the command, so the rest of the line runs.
(( 1/0 )); echo "reached-after-plain"
echo "plain rc=$?"

echo "=== an indirect reference's referent is text, so its subscript is expanded ==="
# `ref` holds `a[$n]`; that `$n` came out of a variable and has never been
# through word expansion, so the shell expands it here.
a=(zero one two three)
m=1
ref='a[$m]'
echo "dollar=[${!ref}]"
ref='a[$(echo 2)]'
echo "cmdsub=[${!ref}]"
ref='a[$((m+2))]'
echo "nested=[${!ref}]"
# A bare name is left for the evaluator, as in any arithmetic string.
ref='a[m]'
echo "bare=[${!ref}] rc=$?"
# And a bad subscript there is a subscript failure like any other: untagged and
# fatal to the rest of the line.
ref='a[1/0]'
echo "[${!ref}]"; echo "unreachable-ref"
echo "badref rc=$?"

echo done
