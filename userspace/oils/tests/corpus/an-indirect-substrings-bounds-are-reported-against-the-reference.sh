# A `${x:off:len}` bound that will not evaluate is reported with the parameter's
# name in front of the arithmetic complaint. Through an indirection that name is
# the *reference the writer typed* — bash renames the part it expands but keeps
# the reference for the diagnostic — so `${!p:b*}` says `!p: b*: syntax error…`
# and never `sv: …`, whatever `p` turned out to point at.
sv=SVAL
n=(a b c d)
declare -A s=([k]=sv) an=([k]='n[@]')
p=sv
pn='n[@]'
declare -n nr=sv
b() { printf '%-22s ' "$1"; c=$2; ( eval "printf '<%s>' $c"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
echo "--- an offset that will not parse"
b '${!p:b*}' '${!p:b*}'
b '${!p:0:b*}' '${!p:0:b*}'
b '${!p:1:b*}' '${!p:1:b*}'
b '${!s[k]:b*}' '${!s[k]:b*}'
b '${!nr:b*}' '${!nr:b*}'
echo "--- and one whose referent is a whole array"
b '${!pn:b*}' '${!pn:b*}'
b '${!an[k]:b*}' '${!an[k]:b*}'
echo "--- a division by zero says the same"
b '${!p:1/0}' '${!p:1/0}'
b '${!p:0:1/0}' '${!p:0:1/0}'
b '${!pn:1/0}' '${!pn:1/0}'
echo "--- an unset name in the bound reports on its own behalf"
( set -u; b '${!p:zz}' '${!p:zz}' )
( set -u; b '${sv:zz}' '${sv:zz}' )
echo "--- with no indirection in front of it the parameter names itself"
b '${sv:b*}' '${sv:b*}'
b '${s[k]:b*}' '${s[k]:b*}'
b '${n[@]:b*}' '${n[@]:b*}'
b '${n[1]:1/0}' '${n[1]:1/0}'
echo "--- a bound that evaluates is untouched"
b '${!p:1:2}' '${!p:1:2}'
b '${!pn:1:2}' '${!pn:1:2}'
echo TAIL
