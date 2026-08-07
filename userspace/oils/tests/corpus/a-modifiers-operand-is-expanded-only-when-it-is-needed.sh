# A `${ … }` modifier's operand words are expanded exactly as often as the
# modifier can use them — which is sometimes never, and for a whole collection
# is once, not once per element.
#
# Three rules, each of them bash's:
#
#   * A substitution and a case modification do nothing to an **unset**
#     parameter, and bash finds that out first: `parameter_brace_patsub` and
#     `parameter_brace_casemod` both open with `if (value == 0) return NULL`
#     (subst.c:9119, 9366). The test is set-ness, not emptiness — `x=''` is a
#     value and its operands do run.
#
#   * A trim turns on the *value* instead: the `#`/`%` arm of
#     `parameter_brace_expand` breaks on `temp == 0 || *temp == '\0'`
#     (subst.c:10084), so an empty-but-set parameter short-circuits too.
#
#   * A bulk operator over `[@]`/`[*]` expands its operands **once** and then
#     walks the elements with the result — `parameter_brace_patsub` computes
#     `pat` and `rep` before calling `array_patsub`. An empty or unset
#     collection expands them no times at all.
#
# None of this is visible in the value; it is visible in the operand's *side
# effects*, which is what the run counts below are for.
#
# Verified against bash 5.2.37.

cnt=${TMPDIR:-/tmp}/oils-operand-runs.$$
trap 'rm -f "$cnt"' EXIT
c() { printf x >> "$cnt"; echo "$1"; }
r() {
  : > "$cnt"
  eval "v=\"$1\"" 2>/dev/null
  printf '%-34s runs=%s val=[%s]\n' "$2" "$(wc -c < "$cnt" | tr -d ' ')" "$v"
}

unset -v u
n=''
s=zaaaz

echo "=== an unset parameter expands no operand ==="
r '${u/$(c a)/Y}'        'replace, pattern'
r '${u/aaa/$(c Y)}'      'replace, replacement'
r '${u//$(c a)/$(c Y)}'  'replace, both'
r '${u^^$(c a)}'         'case ^^'
r '${u,$(c a)}'          'case ,'
r '${u#$(c a)}'          'trim #'
r '${u%$(c a)}'          'trim %'

echo "=== …but an empty one is a value, so a substitution still expands ==="
r '${n/$(c a)/$(c Y)}'   'replace, both'
r '${n^^$(c a)}'         'case ^^'
# The trim is the exception: its test is the value, not set-ness.
r '${n#$(c a)}'          'trim #'
r '${n%%$(c a)}'         'trim %%'

echo "=== a set parameter expands each operand once ==="
r '${s/$(c a)/$(c Y)}'   'replace, both'
r '${s#$(c z)}'          'trim #'
r '${s^^$(c a)}'         'case ^^'
r '${s:$(c 0):$(c 1)}'   'slice, both bounds'

echo "=== a slice is not short-circuited by an empty value, only an unset one ==="
r '${n:$(c 0)}'          'empty, offset'
r '${n:$(c 0):$(c 1)}'   'empty, both'
r '${u:$(c 0)}'          'unset, offset'

echo "=== an operator that supplies a value does expand it ==="
r '${u:-$(c D)}'         'default'
r '${u:+$(c P)}'         'alternate (not taken)'
r '${s:+$(c P)}'         'alternate (taken)'

echo "=== a bulk operator expands its operands once, not once per element ==="
g=(p q p r)
r '${g[@]/$(c p)/Y}'     'replace pattern, 4 elements'
r '${g[@]/p/$(c Y)}'     'replace replacement'
r '${g[@]//$(c p)/$(c Y)}' 'replace both'
r '${g[*]/$(c p)/Y}'     'replace, star'
r '${g[@]#$(c p)}'       'trim #'
r '${g[@]%$(c r)}'       'trim %'
r '${g[@]^^$(c p)}'      'case ^^'

echo "=== …and none at all when the collection has no elements ==="
z=()
unset -v uu
r '${z[@]/$(c p)/Y}'     'empty array, replace'
r '${z[@]#$(c p)}'       'empty array, trim'
r '${z[@]^^$(c p)}'      'empty array, case'
r '${uu[@]/$(c p)/Y}'    'unset array, replace'

echo "=== an element whose value is empty is still an element ==="
o=('')
b=('' '')
r '${o[@]#$(c p)}'       'one empty element, trim'
r '${o[@]/$(c p)/Y}'     'one empty element, replace'
r '${b[@]^^$(c p)}'      'two empty elements, case'

echo "=== a single element read is a scalar, not a bulk op ==="
r '${g[1]/$(c q)/Y}'     'one element, replace'
r '${g[1]#$(c q)}'       'one element, trim'

echo "=== the short-circuit silences the operand's diagnostics too ==="
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }
e 'unset -v u; echo "[${u/aaa/`fi`}]"'
e 'unset -v u; echo "[${u#`fi`}]"'
e 'n=""; echo "[${n#`fi`}]"'
e 'z=(); echo "[${z[@]/aaa/`fi`}]"'
# …and does not silence one it does reach.
e 's=zaaaz; echo "[${s/aaa/`fi`}]"'

echo done
