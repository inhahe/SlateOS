# An element write `X[sub]=v` is a sequence of refusals, and bash's order for
# them is fixed by where each one sits in `assign_array_element`. Nothing about
# the *reference* `X` may be judged before a test that comes earlier.
#
# 1. `array_variable_name` (arrayfunc.c:1413) validates the bracket structure —
#
#      ni = skipsubscript (s, ind, ssflags);
#      if (ni <= ind + 1 || s[ni] != ']')
#        { err_badarraysub (s); … return NULL; }
#
#    — and it runs *before* `find_variable`, so `a[]=9` on a circular chain
#    costs no lookup and warns not at all.
# 2. `entry = find_variable (vname)` (arrayfunc.c:363) — exactly one nameref
#    walk, hence exactly one circular warning.
# 3. the `[*]`/`[@]` refusal (`ALL_ELEMENT_SUB (sub[0]) && sub[1] == ']'`,
#    arrayfunc.c:370) — after the walk, so one warning precedes it.
# 4. `array_expand_index` (arrayfunc.c:398) — arithmetic, so `1+` is a syntax
#    error here.
# 5. the negative-underflow refusal (arrayfunc.c:437).
# 6. only then `bind_array_variable` → `find_variable_nameref_for_create` →
#    `legal_identifier (nameref_cell (var)) == 0` → `sh_invalidid`
#    (variables.c:2276), and below that the readonly refusal.
#
# `legal_identifier` rejects `n[1]`, where the `valid_nameref_value` used when
# the nameref was *created* accepts it — which is how a name can be stored and
# then refused only at the point of use, and how `declare -n r='n[1]'` gets as
# far as being asked about its subscript at all.
#
# Two spellings ride along with the order. Every diagnostic the store makes
# quotes the operand as **written**, because `assign_array_element_internal`
# keeps it for that and nothing else —
#
#      static SHELL_VAR *
#      assign_array_element_internal (entry, name, vname, sub, sublen, …)
#           char *name;		/* only used for error messages */
#
# — while the store itself goes where the chain points. And the walk's *cost*
# is spread over the same order: 0 walks below the bracket test, 1 below the
# subscript's own refusals, the full count only at the bind, which is also the
# only place a reference the store fell back on is unmade.

echo "--- the reference's own refusal is last, under every subscript test"
t1() { ( declare -n r='n[1]'; n=(a b c); r[-1]=9; echo "st=$?"; declare -p n r ); }; t1
t2() { ( declare -n r='n[1]'; n=(a b c); r[1+]=9; echo "st=$?"; declare -p n r ); }; t2
t3() { ( declare -n r='n[1]'; n=(a b c); r[]=9; echo "st=$?"; declare -p n r ); }; t3
t4() { ( declare -n r='n[1]'; n=(a b c); r[*]=9; echo "st=$?"; declare -p n r ); }; t4
t5() { ( declare -n r='n[1]'; n=(a b c); r[@]=9; echo "st=$?"; declare -p n r ); }; t5
t6() { ( declare -n r='n[1]'; n=(a b c); r[x y]=9; echo "st=$?"; declare -p n r ); }; t6
t7() { ( declare -n r='n[1]'; n=(a b c); r[0]=9; echo "st=$?"; declare -p n r ); }; t7
t8() { ( declare -n r='n[1]'; n=(a b c); r[5]=9; echo "st=$?"; declare -p n r ); }; t8
t9() { ( declare -n r='n[1]'; declare -A n; r[k]=9; echo "st=$?"; declare -p n ); }; t9
ta() { ( declare -A m=([k]=1); declare -n r='m[k]'; r[]=9; echo "st=$?" ); }; ta
tb() { ( declare -A m=([k]=1); declare -n r='m[k]'; r[*]=9; echo "st=$?" ); }; tb

echo "--- and the same is so of every other caller that writes by name"
tc() { ( declare -n r='n[1]'; n=(a b c); read 'r[-1]' <<< x; echo "st=$?"; declare -p n r ); }; tc
td() { ( declare -n r='n[1]'; n=(a b c); printf -v 'r[-1]' x; echo "st=$?"; declare -p n r ); }; td
te() { ( declare -n r='n[1]'; n=(a b c); read 'r[]' <<< x; echo "st=$?" ); }; te
tf() { ( declare -n r='n[1]'; n=(a b c); printf -v 'r[]' x; echo "st=$?" ); }; tf
tg() { ( declare -n r='n[1]'; n=(a b c); read 'r[0]' <<< x; echo "st=$?"; declare -p n ); }; tg

echo "--- what a circular chain's walks cost, spread over the same order"
th() { ( declare -n a=b; declare -n b=a; a[]=9; echo "st=$?"; declare -p a b ); }; th
ti() { ( declare -n a=b; declare -n b=a; a[x y]=9; echo "st=$?"; declare -p a b ); }; ti
tj() { ( declare -n a=b; declare -n b=a; a[*]=9; echo "st=$?"; declare -p a b ); }; tj
tk() { ( declare -n a=b; declare -n b=a; a[1+]=9; echo "st=$?"; declare -p a b ); }; tk
tl() { ( declare -n a=b; declare -n b=a; a[-1]=9; echo "st=$?"; declare -p a b ); }; tl
tm() { ( declare -n a=b; declare -n b=a; a[1]=9; echo "st=$?"; declare -p a b ); }; tm
tn() { ( declare -n a=b; declare -n b=a; read 'a[-1]' <<< x; echo "st=$?"; declare -p a b ); }; tn
to() { ( declare -n a=b; declare -n b=a; read 'a[1]' <<< x; echo "st=$?"; declare -p a b ); }; to
tp() { ( declare -n a=b; declare -n b=a; read 'a[]' <<< x; echo "st=$?"; declare -p a b ); }; tp
tq() { ( declare -n a=b; declare -n b=a; printf -v 'a[-1]' x; echo "st=$?"; declare -p a b ); }; tq
tr() { ( declare -n a=b; declare -n b=a; printf -v 'a[1]' x; echo "st=$?"; declare -p a b ); }; tr

echo "--- a chain that escaped the cycle keeps its reference however far it gets"
ts() { f() { local -n g=g; read 'g[-9]' <<< Z; echo "st=$?"; }; ( g=(x y); f; declare -p g ); }; ts
tt() { f() { local -n g=g; read 'g[1]' <<< Z; echo "st=$?"; }; ( g=(x y); f; declare -p g ); }; tt
tu() { f() { local -n g=g; read 'g[]' <<< Z; echo "st=$?"; }; ( g=(x y); f; declare -p g ); }; tu
tv() { f() { local -n g=g; printf -v 'g[-9]' Z; echo "st=$?"; }; ( g=(x y); f; declare -p g ); }; tv
tw() { f() { local -n g=g; read 'g[j]' <<< Z; echo "st=$?"; }; ( declare -A g=([k]=1); f; declare -p g ); }; tw

echo "--- the readonly refusal is the bind's, so it too waits for the subscript"
tx() { ( declare -a q=(1 2); readonly q; read 'q[-9]' <<< x; echo "st=$?" ); }; tx
ty() { ( declare -a q=(1 2); readonly q; declare -n r=q; read 'r[-9]' <<< x; echo "st=$?" ); }; ty
tz() { ( declare -a q=(1 2); readonly q; declare -n r=q; printf -v 'r[-9]' x; echo "st=$?" ); }; tz
u1() { ( declare -a q=(1 2); readonly q; declare -n r=q; read 'r[*]' <<< x; echo "st=$?" ); }; u1
u2() { ( declare -a q=(1 2); readonly q; declare -n r=q; read 'r[1+]' <<< x; echo "st=$?" ); }; u2
u3() { ( q=1; readonly q; declare -n r=q; read 'r[-9]' <<< x; echo "st=$?" ); }; u3
u4() { ( declare -a q=(1 2); readonly q; declare -n r=q; declare -n s=r; read 's[-9]' <<< x; echo "st=$?" ); }; u4

echo "--- …and blames the reference as written, where an unsubscripted one does not"
u5() { ( declare -a q=(1 2); readonly q; declare -n r=q; read 'r[0]' <<< x; echo "st=$?" ); }; u5
u6() { ( declare -a q=(1 2); readonly q; declare -n r=q; printf -v 'r[0]' x; echo "st=$?" ); }; u6
u7() { ( declare -a q=(1 2); readonly q; declare -n r=q; r[0]=9; echo "st=$?" ); }; u7
u8() { ( declare -A q=([k]=1); readonly q; declare -n r=q; echo "${r[j]:=v}"; echo "st=$?" ); }; u8
u9() { ( declare -a q=(1 2); readonly q; declare -n r=q; declare -n s=r; read 's[0]' <<< x; echo "st=$?" ); }; u9
ua() { ( q=1; readonly q; declare -n r=q; read r <<< x; echo "st=$?" ); }; ua
ub() { ( q=1; readonly q; declare -n r=q; printf -v r x; echo "st=$?" ); }; ub

echo "--- a variable the shell maintains is refused at that same bind"
uc() { ( read 'GROUPS[-9]' <<< x; echo "st=$?" ); }; uc
ud() { ( read 'GROUPS[1]' <<< x; echo "st=$?" ); }; ud
ue() { ( printf -v 'GROUPS[-9]' x; echo "st=$?" ); }; ue
uf() { ( read 'LINENO[-9]' <<< x; echo "st=$?" ); }; uf
ug() { ( read 'SECONDS[-9]' <<< x; echo "st=$?" ); }; ug
uh() { ( read 'BASH_ARGV0[-9]' <<< x; echo "st=$?" ); }; uh
