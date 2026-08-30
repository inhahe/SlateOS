# An `@` operator bash will refuse is still an operator. The spelling is judged
# in `parameter_brace_transform`, which a `${!ref[…]@…}` reaches only after
# `parameter_brace_expand_indir` has resolved the pointer and `get_var_and_type`
# has re-read it — so the pointer's subscript runs, the transform's own operand
# never runs at all, and an unset target makes the whole thing empty rather than
# a complaint:
#
#     xc = xform[0];
#     if (value == 0 && xc != 'A' && xc != 'a')
#       return ((char *)NULL);
#     …
#     if (xform[0] == 0 || valid_parameter_transform (xform) == 0)
#       return (interactive_shell ? &expand_param_error : &expand_param_fatal);
#
# The two letters that ask about the *variable* skip that short-circuit, so
# `${nope@aQ}` is refused where `${nope@Za}` is quietly empty. And the refusal is
# fatal: the shell reading it exits.
sv=SVAL
n=(ax by cz)
declare -a ea=()
declare -A s=([k0]=sv)
declare -A u=([k0]=nosuch)
declare -A mt=([k0]='')
declare -A p=([k0]='n[@]')
declare -A pe=([k0]='ea[@]')
declare -A el=([k0]='n[9]')
declare -n gs=sv
declare -n gu=nosuch
# A subshell, because the complaints below end the shell that reads them, and
# `alive` is how that shows.
b() { printf '%-28s ' "$1"; ( eval "printf '<%s>' $1"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
echo "--- what the referent is decides whether the spelling is ever judged"
for v in s u mt p pe el nope; do
  b "\${!$v[k0]@Z}"
  b "\${!$v[k0]@QU}"
  b "\${!$v[k0]@aQ}"
  b "\${!$v[k0]@\$(echo Q)}"
done
echo "--- the same split without an indirection in front of it"
b '${sv@Z}'
b '${nope@Z}'
b '${nope@aQ}'
b '${nope@AQ}'
b '${nope@Za}'
b '${ea[@]@Z}'
b '${ea[@]@aQ}'
b '${ea[*]@aQ}'
b '${n[@]@Z}'
echo "--- a nameref pointer answers with a name, so it is never the unset case"
b '${!gs@Z}'
b '${!gu@Z}'
echo "--- the pointer is read, the operand is not, and the reference is named"
o() { printf '%s ' "$1" >&2; }
f() { o f; echo k0; }
b '${!s[$(f)]@$(o T; echo Q)}'
b '${!u[$(f)]@$(o T; echo Q)}'
b '${!p[$(f)]@$(o T; echo Q)}'
b '${!pe[$(f)]@$(o T; echo Q)}'
b '${!nope[$(f)]@$(o T; echo Q)}'
b '${!u[$(f)]@a$(o T; echo Q)}'
echo "--- and the ones that are not refused come back through the same path"
b '${!s[k0]@Q}'
b '${!p[k0]@Q}'
b '${!u[k0]@a}'
b '${!el[k0]@a}'
echo TAIL
