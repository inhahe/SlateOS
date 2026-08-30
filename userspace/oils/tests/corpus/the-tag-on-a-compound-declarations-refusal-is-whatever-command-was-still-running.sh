# A compound operand's kind refusal is spoken by the *assignment machinery*,
# while the operand is still being expanded — before the declaration builtin has
# become the command that is running. `builtin_error`'s prolog
# (builtins/common.c:88) prints `this_command_name`, so what tags the message is
# whatever command is still running **around** the expansion.
#
# `execute_simple_command` sets that name from the command word at `run_builtin:`
# (execute_cmd.c:4684) — before it knows whether it has a builtin or a function,
# so a call sets it as much as `eval` does — and blanks it on the way out rather
# than putting the old one back (`return_result:`, execute_cmd.c:4828,
# `this_command_name = (char *)NULL; /* points to freed memory now */`).
#
# So the tag is the name of the **call**, and only for as long as no command
# inside it has finished: a function body's first command names the function, a
# second one names nothing at all. It is not the running function by rule, and
# not the builtin's own name — the builtin's half of the same refusal, printed
# after it, carries that.

echo '=== the first command of a body names the call'
( declare -a g=(1); zzz() { readonly -A g=([k]=v); }; zzz )
( declare -a g=(1); zzz() { declare -gA g=([k]=v); }; zzz )
( declare -a g=(1); zzz() { export -A g=([k]=v); }; zzz )

echo '=== anything that ran before it took the name away'
( declare -a g=(1); zzz() { true; readonly -A g=([k]=v); }; zzz )
( declare -a g=(1); zzz() { local q; readonly -A g=([k]=v); }; zzz )
( declare -a g=(1); zzz() { x=$(true); readonly -A g=([k]=v); }; zzz )
( declare -a g=(1); zzz() { : $(true); readonly -A g=([k]=v); }; zzz )

echo '=== a pipeline does not, since bash forks for every element of one'
( declare -a g=(1); zzz() { true | true; readonly -A g=([k]=v); }; zzz )

echo '=== the innermost call is the one that named it'
( declare -a g=(1); i() { readonly -A g=([k]=v); }; o() { i; }; o )
( declare -a g=(1); i() { readonly -A g=([k]=v); }; o() { true; i; }; o )

echo '=== a group keeps it; a loop or a conditional does not'
( declare -a g=(1); zzz() { { readonly -A g=([k]=v); }; }; zzz )
( declare -a g=(1); zzz() { for i in 1; do readonly -A g=([k]=v); done; }; zzz )
( declare -a g=(1); zzz() { if true; then readonly -A g=([k]=v); fi; }; zzz )
( declare -a g=(1); zzz() { while false; do :; done; readonly -A g=([k]=v); }; zzz )

echo '=== `eval` names itself for the first command of its string'
( declare -a g=(1); eval "readonly -A g=([k]=v)" )
( declare -a g=(1); eval "true; readonly -A g=([k]=v)" )
( declare -a g=(1); eval "zzz() { readonly -A g=([k]=v); }; zzz" )

echo '=== at the top of a script there is no command around it at all'
( declare -a g=(1); readonly -A g=([k]=v) )

echo '=== a local target reads the same way, the name being no part of it'
( zzz() { local -a g=(9); readonly -A g=([k]=v); }; zzz )
( zzz() { local -a g=(9); declare -A g=([k]=v); }; zzz )
( zzz() { local -a g=(9); local -A g=([k]=v); }; zzz )
