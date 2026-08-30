# A compound `name=(…)` operand whose name is a *reference* binds through it.
# bash resolves the nameref before it does anything with the operand, so the
# array lands on the target and so does every attribute the command names —
# the reference itself is left exactly as it was, still a reference.
#
# The refusals are grouped through `e` rather than redirected per command,
# because osh emits some of them before the command's own redirections are in
# place (TD-OILS-DECL-DIAGNOSTIC-ESCAPES-REDIRECTION).
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo -n "$1 -> "; declare -p "$2" 2>&1; }

echo "=== the kind letters convert the target, not the reference"
{ declare -A n1=([k]=v); declare -n r1=n1; declare -A r1=(z); echo "rc=$?"; p 'n1' n1; p 'r1' r1; } 2>&1 | e
{ declare -a n2=(1); declare -n r2=n2; declare -a r2=(z); echo "rc=$?"; p 'n2' n2; p 'r2' r2; } 2>&1 | e

echo "=== …and they convert a scalar or create an unset one"
{ n3=s; declare -n r3=n3; declare -a r3=(z); echo "rc=$?"; p 'n3' n3; p 'r3' r3; } 2>&1 | e
{ declare -n r4=n4; declare -A r4=([k]=z); echo "rc=$?"; p 'n4' n4; p 'r4' r4; } 2>&1 | e

echo "=== the value attributes follow too, so the literal binds under them"
{ n5=1; declare -n r5=n5; declare -i r5=(2+3); echo "rc=$?"; p 'n5' n5; p 'r5' r5; } 2>&1 | e
{ n6=1; declare -n r6=n6; declare -l r6=(AB); echo "rc=$?"; p 'n6' n6; p 'r6' r6; } 2>&1 | e

echo "=== so do the ones the builtin applies afterwards"
{ n7=1; declare -n r7=n7; declare -x r7=(5); echo "rc=$?"; p 'n7' n7; p 'r7' r7; } 2>&1 | e
{ n8=1; declare -n r8=n8; declare -t r8=(z); echo "rc=$?"; p 'n8' n8; p 'r8' r8; } 2>&1 | e
{ n9=1; declare -n r9=n9; readonly r9=(z); echo "rc=$?"; p 'n9' n9; p 'r9' r9; } 2>&1 | e
{ na=1; declare -n ra=na; export ra=(z); echo "rc=$?"; p 'na' na; p 'ra' ra; } 2>&1 | e

echo "=== +n is the exception: it unmakes the reference it bound through"
{ nb=1; declare -n rb=nb; declare +n rb=(z); echo "rc=$?"; p 'nb' nb; p 'rb' rb; } 2>&1 | e

echo "=== a chain is followed all the way to the end"
{ nc=1; declare -n mc=nc; declare -n rc=mc; declare -a rc=(z); echo "rc=$?"; p 'nc' nc; p 'mc' mc; p 'rc' rc; } 2>&1 | e

echo "=== the builtin's own refusals name the operand as it was written"
{ declare -A nd=([k]=v); declare -n rd=nd; declare -aA rd=(z); echo "rc=$?"; p 'nd' nd; p 'rd' rd; } 2>&1 | e
{ declare -a ne=(1); declare -n re=ne; declare +a re=(z); echo "rc=$?"; p 'ne' ne; p 're' re; } 2>&1 | e
# …including the untagged conversion refusal, which discards the rest of the
# parse unit rather than merely failing — hence the `echo` that never runs.
{ declare -a nf=(1); declare -n rf=nf; declare -A rf=([k]=v); echo "unreached"; } 2>&1 | e

echo "=== but a refusal about where the binding lands names the target"
# …and loses the function tag the direct form carries, because the refusal is
# raised from the resolution rather than from the operand word.
fg1() { local -a GROUPS=(z); echo "rc=$?"; }
fg1 2>&1 | e
fg2() { local -n g=GROUPS; local -a g=(z); echo "rc=$?"; }
fg2 2>&1 | e
readonly nro=1
fr1() { local -a nro=(z); echo "rc=$?"; }
fr1 2>&1 | e
fr2() { local -n r=nro; local -a r=(z); echo "rc=$?"; }
fr2 2>&1 | e

echo "=== a local reference onto a global makes the *target* a local"
gt=1
fl() { local -n r=gt; declare -a r=(z); echo "rc=$?"; p 'r' r; }
fl 2>&1 | e
p 'gt' gt

echo "=== but a local binding does not follow a *global* reference"
# What decides is which binding the declaration writes, not what the name
# resolves to from where it stands: a frame that is about to make its own `r`
# merely shadows the global reference of that name.
gw=5; declare -n gr=gw
fs() { declare -a gr=(z); echo "rc=$?"; p 'gr' gr; }
fs 2>&1 | e
p 'gw' gw; p 'gr' gr

echo "=== a reference that resolves to nothing falls back to its own name"
# A cycle warns once, then the literal binds to the name it was written with —
# which stops being a reference, since no variable is both.
{ declare -n c1=c2; declare -n c2=c1; declare -a c1=(z); echo "rc=$?"; p 'c1' c1; p 'c2' c2; } 2>&1 | e
# A reference that never became one (its target was not a name) is not a
# reference at all, so there is nothing to follow.
{ declare -n r10=; declare -a r10=(z); echo "rc=$?"; p 'r10' r10; } 2>&1 | e
{ declare -n r11='1x'; declare -a r11=(z); echo "rc=$?"; p 'r11' r11; } 2>&1 | e

echo "=== a scalar operand's refusals split the same way, name for name"
# Raised against the word: the assignment's readonly refusal and the kind
# conversion.
{ readonly s1=1; declare -n q1=s1; declare q1=9; echo "rc=$?"; } 2>&1 | e
{ declare -A s2=([k]=v); declare -n q2=s2; declare -a q2=z; echo "rc=$?"; } 2>&1 | e
{ declare -a s3=(1); declare -n q3=s3; declare -A q3; echo "rc=$?"; } 2>&1 | e
{ declare -a s4=(1); declare -n q4=s4; declare +a q4; echo "rc=$?"; } 2>&1 | e
# Raised against the resolved name: the shadow refusals and `+r`.
{ declare -n q5=s1; declare +r q5; echo "rc=$?"; } 2>&1 | e
fq1() { local -n q=s1; local q; echo "rc=$?"; }
fq1 2>&1 | e
fq2() { local -n q=GROUPS; local q; echo "rc=$?"; }
fq2 2>&1 | e
# …and the `-aA` self-conflict is raised against whichever of the two the
# array it refuses belongs to: one the command *found* is the operand's, one
# the command just *made* is the target's.
{ declare -A s6; declare -n q6=s6; declare -aA q6; echo "rc=$?"; } 2>&1 | e
{ declare -n q7=s7; declare -aA q7; echo "rc=$?"; } 2>&1 | e
# A compound literal binds during word expansion, so its array is always one
# the builtin found — even when the target did not exist before the command.
{ declare -n q8=s8; declare -aA q8=(z); echo "rc=$?"; p 's8' s8; } 2>&1 | e

echo "=== an element reference is not a name a literal can bind to"
{ declare -a ng=(1 2); declare -n rg='ng[1]'; declare -a rg=(z); echo "unreached"; } 2>&1 | e

echo "=== done"
