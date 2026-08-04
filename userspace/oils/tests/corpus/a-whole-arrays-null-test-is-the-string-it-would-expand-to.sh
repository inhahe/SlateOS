# The colon forms of `${a[@]:-d}` ask whether the array is *null*, and the
# string they ask about is the one the reference would expand to right there —
# so the answer follows `$IFS` and the quotes around it. An array of two empty
# elements is null only when the join has nothing to put between them, which is
# a quoted `[*]` under an empty `$IFS`: outside quotes a `[*]` expands like a
# `[@]` (both are about to be split), and a `[@]` glues with a space whatever
# `$IFS` says.
#
# Every separator other than the empty one is a character the test can see, so
# the difference hides completely under the default `$IFS` — and under any
# other non-empty one. It takes `IFS=` to show it.

a0=(); a1=(""); a2=("" ""); a3=("" "" ""); b=("" x); c=(x)

show() { printf '  %-16s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

# `q` reports what a *quoted* reference decided, `u` what an unquoted one did.
# The field count is the point: an inactive `:+` contributes nothing to an
# unquoted word, and its `A` is one field when it fired.
q() { printf ' %s=%s' "$1" "${2:+A}"; }
u() { printf ' %s=%s' "$1" ${2:+A}; }

echo "### the four spellings, under three separators"
for ifs in ' ' ':' ''; do
  IFS=$ifs
  printf '  q  [%s] ' "$ifs"
  printf '%s' "a0=${a0[*]:+A}/${a0[@]:+A}"
  printf ' %s' "a1=${a1[*]:+A}/${a1[@]:+A}"
  printf ' %s' "a2=${a2[*]:+A}/${a2[@]:+A}"
  printf ' %s' "a3=${a3[*]:+A}/${a3[@]:+A}"
  printf ' %s' "b=${b[*]:+A}/${b[@]:+A}"
  printf ' %s\n' "c=${c[*]:+A}/${c[@]:+A}"
  IFS=' '
done
IFS=' '

echo "### and unquoted, where the star is a star no longer"
IFS=
show 'a2[*] unq'  ${a2[*]:+A}
show 'a2[*] q'    "${a2[*]:+A}"
show 'a2[@] unq'  ${a2[@]:+A}
show 'a2[@] q'    "${a2[@]:+A}"
show 'a3[*] unq'  ${a3[*]:+A}
show 'a3[*] q'    "${a3[*]:+A}"
show 'a1[*] unq'  ${a1[*]:+A}
show 'a1[*] q'    "${a1[*]:+A}"
IFS=' '

echo "### the same question asked by :- and :?"
IFS=
show 'a2[*]:- unq' ${a2[*]:-d}
show 'a2[*]:- q'   "${a2[*]:-d}"
show 'a2[@]:- unq' ${a2[@]:-d}
show 'a2[@]:- q'   "${a2[@]:-d}"
show 'a3[*]:- q'   "${a3[*]:-d}"
# The `:?` form answers the same question by dying, so the separator decides
# whether the command runs at all.
(echo "  a2[*]:? <${a2[*]:?null}>"); echo "    rc=$?"
(echo "  a2[@]:? <${a2[@]:?null}>"); echo "    rc=$?"
IFS=' '

echo "### the colon-less forms never ask, so no separator reaches them"
IFS=
show 'a2[*]-d q'  "${a2[*]-d}"
show 'a2[@]-d q'  "${a2[@]-d}"
show 'a1[*]+A q'  "${a1[*]+A}"
show 'a0[*]+A q'  "${a0[*]+A}"
IFS=' '

echo "### which context counts is the word's own, not the command's"
IFS=
x=${a2[*]:+A};   printf '  assign unq  <%s>\n' "$x"
x="${a2[*]:+A}"; printf '  assign q    <%s>\n' "$x"
case ${a2[*]:+A} in A) echo '  case unq    A';; *) echo '  case unq    -';; esac
case "${a2[*]:+A}" in A) echo '  case q      A';; *) echo '  case q      -';; esac
[[ ${a2[*]:+A} == A ]] && echo '  dbr unq     A' || echo '  dbr unq     -'
[[ "${a2[*]:+A}" == A ]] && echo '  dbr q       A' || echo '  dbr q       -'
printf '  heredoc     '; cat <<EOF
<${a2[*]:+A}>
EOF
IFS=' '

echo "### a substitution's own words are not inside the quotes around it"
IFS=
printf '  cmdsub unq  <%s>\n' "$(echo ${a2[*]:+A})"
printf '  cmdsub q    <%s>\n' "$(echo "${a2[*]:+A}")"
IFS=' '

echo "### but an operand is: the quotes reach what it expands in turn"
IFS=
show 'operand q'   "${a0[@]:-${a2[*]:+A}}"
show 'operand unq' ${a0[@]:-${a2[*]:+A}}
IFS=' '
