# bash draws no line between a scalar it stores and one it *computes*. Both are
# a `SHELL_VAR`, and a subscripted read reaches the same value cell —
# `t = (ind == 0) ? value_cell (var) : NULL` for `[0]`, and
# `return (var_isset (var) ? 1 : 0)` for `[@]`/`[*]`. So the names answered by a
# value function (`SECONDS`, `LINENO`, `PPID`, `BASHPID`, `RANDOM`,
# `EPOCHSECONDS`, `BASH_SUBSHELL`, `BASH_ARGV0`, …) behave under a subscript
# exactly as a plain `s=hi` does: `${s[0]}` is the value, `${s[@]}` is the
# one-element list, `${!s[@]}` is `0`, `${#s[@]}` is 1, and every operator
# layered on top of them — `+`, `:-`, `@Q`, a pattern, a slice, arithmetic —
# reads through the same cell.
#
# Reading through a subscript is a *read*, so it has the read's side effects
# too: the value function runs, and it runs once per read. `${RANDOM[0]}` and
# `$(( RANDOM[0] ))` each draw a number, taking the sequence the unsubscripted
# spellings would have taken from the same seed.
#
# The three ways the binding can stop being the dynamic one are unchanged by the
# subscript, because they are decisions about the *variable*, not the read: a
# `local` of the name shadows it with an ordinary (and initially unset) one, an
# assignment prefix shadows it with a valued one, and `unset` drops the value
# function for good.
#
# Two questions are *not* about the value and so do not soften: a negative
# subscript still has nothing to count back from on a scalar
# (`${SECONDS[-1]}` is `bad array subscript`), and a subscripted *length* still
# asks about the array shape, which a scalar has none of — so under `set -u`
# `${#SECONDS[0]}` faults naming the base alone, exactly as `${#s[0]}` does for
# an ordinary scalar. See
# `a-word-is-abandoned-at-its-first-fault-and-says-nothing-more`.
#
# The value-carrying probes below compare a special against *itself* rather than
# printing it, because a pid, a path and a clock differ between the two shells;
# and `SECONDS` is never printed, only counted, since it can tick between two
# reads.

# TIMEOUT: 60

p() { printf '%-44s ' "$1"; ( eval "$1"; echo "in=$?" ) 2>&1; echo "  sub=$?"; }

echo '=== the one-element array a scalar has been all along'
for n in SECONDS LINENO PPID BASHPID EPOCHSECONDS BASH_SUBSHELL BASH_ARGV0; do
  printf '%-16s ' "$n"
  eval 'printf "count=%s keys=%s set=%s outofrange=%s\n" \
        "${#'"$n"'[@]}" "${!'"$n"'[@]}" "${'"$n"'[0]+SET}" "${'"$n"'[1]-NONE}"'
done

echo '=== and it is the same value the unsubscripted spelling gives'
for n in BASHPID BASH_SUBSHELL BASH_ARGV0; do
  printf '%-16s ' "$n"
  eval 'if [ "${'"$n"'[0]}" = "$'"$n"'" ] && [ "${'"$n"'[*]}" = "$'"$n"'" ] &&
           [ "${'"$n"'[@]}" = "$'"$n"'" ]
        then echo same; else echo DIFFERENT; fi'
done
# …and arithmetic reads it through the same cell, so a numeric one compares
# equal to itself there too.
for n in BASHPID BASH_SUBSHELL; do
  printf '%-16s ' "$n"
  eval 'echo "arith-same=$(( '"$n"'[0] == '"$n"' ))"'
done

echo '=== a computed value is drawn afresh by the subscripted read too'
case ${RANDOM[0]} in '' | *[!0-9]*) echo 'RANDOM[0] not numeric' ;; *) echo 'RANDOM[0] numeric' ;; esac
case $(( RANDOM[0] )) in *[!0-9]*) echo 'arith RANDOM[0] not numeric' ;; *) echo 'arith RANDOM[0] numeric' ;; esac
# *How many* numbers a subscripted read draws is deliberately not matched — it
# counts bash's `find_variable` calls rather than anything semantic, and the
# count is different for every operator. See known-issues.md
# TD-OILS-A-SUBSCRIPTED-READ-DRAWS-A-DIFFERENT-NUMBER-OF-TIMES. What is asserted
# here is only that the draws come from the seeded sequence at all, which is
# checkable without printing a number: from one seed the stream never repeats.
RANDOM=1; a=${RANDOM[0]}; b=${RANDOM[0]}; c=${RANDOM[*]}
if [ "$a" != "$b" ] && [ "$b" != "$c" ]; then
  echo 'each subscripted read draws afresh'
else
  echo "REPEATED: $a $b $c"
fi

echo '=== operators read through the same cell'
p 'echo "${BASH_SUBSHELL[@]@Q} ${BASH_SUBSHELL[0]@Q}"'
p 'echo "${BASH_SUBSHELL[0]/0/Z}|${BASH_SUBSHELL[0]#0}|${BASH_SUBSHELL[0]:-D}"'
p 'echo "${BASH_SUBSHELL[@]:0:1}|${BASH_SUBSHELL[0]:0:1}"'
p 'n=BASH_SUBSHELL; echo "${!n[0]}|${!n[@]}"'
p 'declare -n r=BASH_SUBSHELL; echo "${r[0]}|${#r[@]}|${!r[@]}"'
p 'echo "${LINENO[0]}|${!LINENO[@]}|${#LINENO[@]}"'

echo '=== the three ways the dynamic binding stops being the one read'
p 'f(){ local SECONDS; echo "${SECONDS[0]-UNSET}|${!SECONDS[@]}|${#SECONDS[@]}"; }; f'
p 'f(){ local SECONDS=9; echo "${SECONDS[0]}|${!SECONDS[@]}"; }; f'
p 'SECONDS=100 eval "echo \${SECONDS[0]}|\${!SECONDS[@]}"'
p 'unset SECONDS; echo "[${SECONDS[0]-UNSET}][${!SECONDS[@]}][${#SECONDS[@]}]"'
p 'unset RANDOM; echo "[${RANDOM[0]-UNSET}][${!RANDOM[@]}]"'
p 'unset RANDOM; RANDOM=7; echo "[${RANDOM[0]}][${RANDOM[@]}][${!RANDOM[@]}]"'

echo '=== a scalar has no highest index and no shape, computed or not'
p 'echo "[${SECONDS[-1]-NONE}]"'
p 'echo "[${#SECONDS[-1]}]"'
p 's=hi; echo "[${s[-1]-NONE}]"'
set -u
p 'echo "[${SECONDS[0]}]" >/dev/null; echo reached'
p 'echo "[${SECONDS[1]}]"'
p 'echo "[${#SECONDS[0]}]"'
p 'echo "[${#SECONDS[@]}]"'
p 's=hi; echo "[${#s[0]}]"'
set +u

echo '=== done'
