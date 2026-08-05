# bash reads an alias by *pushing its replacement onto the input*. `push_string`
# (parse.y) sets `shell_input_line` to the replacement outright and stacks the
# line it displaced; `pop_string` puts that line back when the reader passes the
# replacement's end. So while the reader is inside one, "the current input line"
# — the thing every `syntax error near …' slice and every echoed source line is
# taken from — *is* the alias's value, not the line the alias was written on.
#
# That makes the same `;' get blamed on two different texts, decided by nothing
# but whether anything follows it inside the alias:
#
#   alias A="[[ P;Q";  A ]]      near `;Q'   line `[[ P;Q'
#   alias A="[[ P ;";  A Q ]]    near `A'    line `A Q ]]'
#
# In the second the `;' is the replacement's last character, so the look past it
# is the read that finds the pushed string used up and pops — and the error is
# reported against the written line, at the offset just past the alias word.
#
# The line *number* is the alias word's throughout: a replacement is not a line
# of the script and has none of its own.

shopt -s expand_aliases
s() { sed 's/^.*: line /line /'; }
r() { "$BASH" --norc -c "shopt -s expand_aliases
$1" 2>&1 | s; }

echo "=== inside the replacement, so the replacement is what is quoted"
r 'alias A="[[ P;Q"
A ]]'
r 'alias A="[[ P;Q ]]"
A'
r 'alias A="[[ P;Q ]] ; echo after"
A'
r 'alias A="echo one; [[ P;Q"
A ]]'
r 'alias A="echo a; [[ Z;Y ]]; echo b"
A'

echo "=== the replacement runs out first, so the reader pops back to the line"
# `[[ P ;' ends on the offending token, `echo a ;;' and `echo a )' likewise —
# each needs one more read, and that read is the pop.
r 'alias A="[[ P ;"
A Q ]]'
r 'alias A="echo a ;;"
A'
r 'alias A="echo a ;;"
A tail'
r 'alias A="echo a )"
A'

echo "=== the same tokens one character short of the end stay inside it"
r 'alias A="echo a ;; echo b"
A'
r 'alias A="echo a ) more"
A'
r 'alias A="if ;; then"
A'

echo "=== an operator that never looks again does not pop, even flush at the end"
# `>>' is completed by its own lookahead and returns right then, so the reader is
# parked inside the replacement rather than past it.
r 'alias A="[[ a>>"
A b ]]'
r 'alias A="[[ a>>b ]]"
A'

echo "=== the offending token came from the written line after all"
r 'alias A="[["
A P;Q ]]'
r 'alias A="[[ P"
A ;Q ]]'
r 'alias A="[[ -n"
A ]]'

echo "=== a newline is not part of the replacement, which has no lines of its own"
r 'alias A="[[ P"
A
]]'

echo "=== through another alias, and through a capture"
r 'alias B="[[ P;Q"
alias A="B"
A ]]'
r 'alias A="[[ P;Q ]]"
x=$(A)'

echo "=== and the slice is still bash's textual one, not the token"
r 'alias A="[[ -n @(x) ]]"
A'

echo "=== none of which disturbs an alias that parses"
r 'alias sudo="echo S "
alias ls="echo L"
sudo ls'
r 'alias a="shopt -s expand_aliases; b"
alias b="echo B"
a'
r 'alias A="echo x"
A
A'
echo done
