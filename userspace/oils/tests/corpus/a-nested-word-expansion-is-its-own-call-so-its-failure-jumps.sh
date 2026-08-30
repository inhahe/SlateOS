# Every entry into `expand_word_internal` but one is *nested*, and the facts
# that follow from that are visible in the same places.
#
# `call_expand_word_internal` is the entry every caller but the prompt uses, and
# it answers the two error returns with a jump:
#
#     if (tresult == &expand_word_error || tresult == &expand_word_fatal)
#       { ... exp_jump_to_top_level ((tresult == &expand_word_error)
#                                     ? DISCARD : FORCE_EOF); }
#                                                     /* subst.c:4326-4334 */
#
# and `exp_jump_to_top_level` ends in `jump_to_top_level (v)` unconditionally
# (subst.c:12141-12157) — it never reads `no_longjmp_on_fatal_error`. So:
#
# * **The failure jumps.** `expand_prompt_string` keeps its *return value*
#   (subst.c:4463-4467), so only a failure raised on the prompt's own walk comes
#   back as text. One raised inside a pattern, an operand, a replacement, a
#   subscript or an arithmetic string longjmps straight past that frame and the
#   command is abandoned — under `${x@P}` exactly as anywhere else.
# * **It names its own word.** The `WORD_DESC` the nested call is handed is the
#   pattern/operand/replacement alone, so a "bad substitution" met in there
#   quotes that and not the `${ … }` around it.
# * **Nothing after it in the enclosing construct happens.** Whatever the
#   construct was going to do with the string it never got — store a `:=`
#   default, judge an empty associative key, print a `${x:?word}` message — is
#   downstream of the jump.
#
# `$(( … ))` is the same rule read twice over. `expand_arith_string` scans for
# `ARITH_EXP_CHAR` — `$`, a backquote, `CTLESC`, `~` (subst.c:3824) — and only
# when it finds one does it call `call_expand_word_internal` (subst.c:4033-4051);
# the `evalexp` afterwards is the *caller's* own frame either way. So an
# arithmetic error is interceptable and a bad substitution inside the same
# expression is not. And that `evalexp` runs with no name tag at all
# (`savecmd = this_command_name; this_command_name = (char *)NULL;`,
# subst.c:10611-10612), which shows wherever the substitution sits inside
# something that does carry one — a `${s:off:len}` bound being the one such
# place.
#
# One last thing follows from the jump, and outlives it. `expand_prompt_string`
# lowers the flag on the line *after* the expansion returns:
#
#     no_longjmp_on_fatal_error = 1;
#     value = expand_word_internal (&td, quoted, 0, …);
#     no_longjmp_on_fatal_error = 0;               /* subst.c:4459-4461 */
#
# so a failure that jumped past that line never lowers it, and it stays raised
# for the rest of the shell's life. That shows wherever the flag is read
# *directly* — chiefly `array_expand_index` (arrayfunc.c:1372), which from then
# on answers a bad subscript with 0 instead of jumping. The two error returns
# are unaffected, `call_expand_word_internal` turning them into jumps without
# consulting it.
#
# Verified against bash 5.2.37.

y=Y
s=abcdef
q=QQ
a=(0 1 2)
z=3
e=
declare -A m=([k]=K)

echo "=== the prompt's own walk keeps its text"
v='A$((1+))B';         printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A$((1+$z+))B';      printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${s:1+:2}B';       printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${a[0p]}B';        printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${#a[1+]}B';       printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${a[$z p]}B';      printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${q!}B';           printf '[%s] rc=%s\n' "${v@P}" "$?"

echo "=== a pattern is a nested call, so its failure jumps"
v='A${y#$((1+))}B';    printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${y#[$((1+))]}B';  printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${y%$((1+))}B';    printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${y^$((1+))}B';    printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${y//$((1+))/z}B'; printf '[%s] rc=%s\n' "${v@P}" "$?"

echo "=== so are a replacement, an operand, a bound and a subscript"
v='A${y//Y/$((1+))}B'; printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${u:-$((1+))}B';   printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${u:=$((1+))}B';   printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${q:+$((1+))}B';   printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${s:$((1+)):1}B';  printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${a[$((1+))]}B';   printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${m[$((1+))]}B';   printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${#a[$((1+))]}B';  printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${!a[$((1+))]}B';  printf '[%s] rc=%s\n' "${v@P}" "$?"

echo "=== a bad substitution in the same positions, likewise"
v='A${y#${q!}}B';      printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${y//Y/${q!}}B';   printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${s:${q!}:1}B';    printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${m[${q!}]}B';     printf '[%s] rc=%s\n' "${v@P}" "$?"
v='A${u:-${q!}}B';     printf '[%s] rc=%s\n' "${v@P}" "$?"

echo "=== a nounset abort raised inside one jumps too"
( set -u; v='A${y#$nope}B';  printf '[%s] rc=%s\n' "${v@P}" "$?" ) 2>&1; echo "sub=$?"
( set -u; v='A$nope B';      printf '[%s] rc=%s\n' "${v@P}" "$?" ) 2>&1; echo "sub=$?"

echo "=== the nested call names its own word"
( echo "A${y//Y/${q!}}B" ) 2>&1;  echo "sub=$?"
( echo "A${y%%${q!}}B" ) 2>&1;    echo "sub=$?"
( echo "A${u:-${q!}}B" ) 2>&1;    echo "sub=$?"
( echo "A${m[${q!}]}B" ) 2>&1;    echo "sub=$?"
( echo "A${s:${q!}:1}B" ) 2>&1;   echo "sub=$?"

echo "=== and what the construct was going to do next never happens"
( echo "A${u:=$((1+))}B"; echo "u=${u-unset}" ) 2>&1;  echo "sub=$?"
( echo "A${u:?$((1+))}B" ) 2>&1;  echo "sub=$?"
( echo "A${u:?${q!}}B" ) 2>&1;    echo "sub=$?"
( echo "A${m[${q!}]}B" ) 2>&1;    echo "sub=$?"
( echo "A${#m[${q!}]}B" ) 2>&1;   echo "sub=$?"

echo "=== the same, written so the expansion succeeds"
( echo "A${u:=$e d}B"; echo "u=${u-unset}" ) 2>&1;  echo "sub=$?"
( echo "A${w:?$e boom}B" ) 2>&1;  echo "sub=$?"
( echo "A${m[$e]}B" ) 2>&1;       echo "sub=$?"
( echo "A${#m[$e]}B" ) 2>&1;      echo "sub=$?"

echo "=== an arithmetic substitution evaluates with no name tag"
( echo "A${s:1+:1}B" ) 2>&1;      echo "sub=$?"
( echo "A${s:$((1+)):1}B" ) 2>&1; echo "sub=$?"
( echo "A$((1+))B" ) 2>&1;        echo "sub=$?"

echo "=== a prompt that jumped leaves no_longjmp_on_fatal_error raised"
( echo "A${a[1+]}B" ) 2>&1;         echo "sub=$?"
( eval 'echo "A${a[2 x]}B"' ) 2>&1; echo "sub=$?"
echo "A${a[1+]}B"; echo "rc=$?"
( set -u; echo "A${nope}B" ) 2>&1;  echo "sub=$?"
( echo "A${q!}B" ) 2>&1;            echo "sub=$?"
( eval 'echo "A$(fi)B"' ) 2>&1;     echo "sub=$?"
( eval 'echo "A${y:-b"' ) 2>&1;     echo "sub=$?"
( eval 'echo "A$((1+"' ) 2>&1;      echo "sub=$?"

echo done
