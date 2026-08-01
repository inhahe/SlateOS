# `$_` — the last word of the previous simple command.
#
# The binding happens when the command *ends*, so a command's own body still
# reads the previous value and then has its binding overwritten; and a command
# with no words at all binds the empty string.
echo a b c
printf 'after echo   |%s|\n' "$_"
v=x
printf 'after assign |%s|\n' "$_"
> /dev/null
printf 'after redir  |%s|\n' "$_"
x=$(: cs)
printf 'after cmdsub |%s|\n' "$_"

f() { printf 'inside f     |%s|\n' "$_"; : inner; }
: A; f zz
printf 'after f      |%s|\n' "$_"
: A; eval ': ev'
printf 'after eval   |%s|\n' "$_"

: A; true
printf 'bare builtin |%s|\n' "$_"
: A; nosuchcmd zz 2>/dev/null
printf 'not found    |%s|\n' "$_"

# A compound command is not a simple command: its body's binding stands, and a
# subshell's does not escape.
: A; { : br; }
printf 'after group  |%s|\n' "$_"
: A; (: sub)
printf 'after sub    |%s|\n' "$_"
: A; if :; then :; fi
printf 'after if     |%s|\n' "$_"
: A; [[ -n x ]]
printf 'after cond   |%s|\n' "$_"

: A; unset _
printf 'after unset  |%s|\n' "$_"
: A; _=custom
printf 'after _=      |%s|\n' "$_"
: A; _=custom :
printf 'prefix _=     |%s|\n' "$_"
echo "=== done"
