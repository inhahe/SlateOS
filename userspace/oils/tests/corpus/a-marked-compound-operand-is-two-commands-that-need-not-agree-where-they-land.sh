# A compound operand of a declaration builtin is not one command but three.
# bash's `expand_declaration_argument` (subst.c:12653) decomposes `g=(1 2)` into
#
#   1. `declare <opts> g`      — the builtin over the word truncated at the `=`,
#   2. the compound assignment itself, and
#   3. the word truncated to the name again, handed to the **real builtin**
#      after the whole word-expansion pass has finished.
#
# For `declare`/`local`/`typeset` that is bookkeeping: all three name the same
# variable. For `readonly` and `export` it is observable, because those two are
# not LOCALVAR_BUILTINs. Every assignment word of theirs is stamped
# `W_ASSNGLOBAL|W_CHKLOCAL` (execute_cmd.c:4221), so steps 1 and 2 run as though
# `declare -gG` had been written — the *value* goes to the frame's own local if
# the reference reaches one, and to the **global** otherwise. Step 3 carries no
# such stamp. It runs later, outside that arrangement, and marks whatever the
# name is bound to *then*, followed through its reference chain.
#
# So the array and the attribute can land on two different variables:
#
#      f() { local -n g=z; readonly g=(1 2); }
#      the value  -> the global `g`, because the local `g` is a nameref, not a
#                    local the chklocal rule would accept
#      the `-r`   -> `z`, because that is where the live reference points
#
# `-a`/`-A` are not passed on to step 3: they shape an operand that carries a
# value, and the truncated one carries none. What a kind letter does do is make
# the refusal fatal. `W_FORCELOCAL` (command.h:105) is set only on an assignment
# builtin that is *also* a LOCALVAR_BUILTIN inside a function, and it is the sole
# thing standing between a non-fatal error and `exp_jump_to_top_level (DISCARD)`
# at subst.c:12762 — so `declare -ga` in a function reports twice and carries on,
# while `readonly -a`/`export -a` throw the rest of the parse unit away wherever
# they run.

echo '=== the value goes global, the attribute follows the reference'
( f() { local -n g=z; readonly g=(1 2); declare -p g z 2>&1; }; f; echo AFTER; declare -p g z 2>&1 )
( f() { local -n g=z; export g=(1 2); declare -p g z 2>&1; }; f; echo AFTER; declare -p g z 2>&1 )

echo '=== the declare family keeps both halves together'
( f() { local -n g=z; declare g=(1 2); declare -p g z 2>&1; }; f; echo AFTER; declare -p g z 2>&1 )
( f() { local -n g=z; declare -g g=(1 2); declare -p g z 2>&1; }; f; echo AFTER; declare -p g z 2>&1 )

echo '=== a chain is followed to its end, and only its end is marked'
( f() { local -n g=h2; local -n h2=z; export g=(1 2); declare -p g h2 z 2>&1; }; f; echo AFTER; declare -p g h2 z 2>&1 )

echo '=== a global nameref parts nothing: chklocal finds no local either way'
( declare -n g=z; f() { readonly g=(1 2); declare -p g z 2>&1; }; f; echo AFTER; declare -p g z 2>&1 )
( declare -n g=z; readonly g=(1 2); declare -p g z 2>&1 )

echo '=== nor does a local the reference actually reaches'
( f() { local z=Z; local -n g=z; readonly g=(1 2); declare -p g z 2>&1; }; f; echo AFTER; declare -p g z 2>&1 )

echo '=== every operand is split the same way, compound or not'
( f() { local -n g=z; readonly g=(1 2) s=5 w=(3); declare -p g z s w 2>&1; }; f; echo AFTER; declare -p g z s w 2>&1 )

echo '=== a kind letter is fatal for readonly/export, not for the declare family'
( declare -A m=([a]=1); f() { declare -ga m=(1 2); echo "reached rc=$?"; }; f; echo "out=$?" )
( declare -A m=([a]=1); f() { readonly -a m=(1 2); echo "reached rc=$?"; }; f; echo "out=$?" )
( declare -A m=([a]=1); f() { export -a m=(1 2); echo "reached rc=$?"; }; f; echo "out=$?" )
( declare -A m=([a]=1); readonly -a m=(1 2); echo "reached rc=$?" )
( declare -A m=([a]=1); f() { local -a m=(1 2); echo "reached rc=$?"; }; f; echo "out=$?" )

echo '=== a subshell of its own does not change where either half lands'
( f() { local -n g=z; ( readonly g=(1 2); declare -p g z 2>&1 ); declare -p g z 2>&1; }; f )

echo '=== marking twice is marking the same place twice'
( f() { local -n g=z; readonly g=(1 2); readonly g=(3); declare -p g z 2>&1; }; f; echo "rc=$?" )
