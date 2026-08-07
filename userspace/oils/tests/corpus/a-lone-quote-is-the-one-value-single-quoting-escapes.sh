# bash has exactly one function that writes a value back as a single-quoted
# shell word — `sh_single_quote` (lib/sh/shquote.c:95) — and reaches it from
# four directions that have nothing else in common: `${v@Q}`/`${v@A}` (subst.c),
# `alias` and `trap -p` printing a definition back (alias.c, trap.c),
# `declare -f` re-quoting a word (print_cmd.c), and history's `:q` modifier
# (readline's histexpand.c:847).
#
# Its general rule is the familiar one: a quote cannot live inside a quoted run,
# so it is lifted out and written `'\''` — close, escape, reopen. But a value
# that is *nothing but* a quote is special-cased (shquote.c:103) to the two
# characters `\'`, because the general rule would spend six characters writing
# one. The test is `string[0] == '\'' && string[1] == 0`: the whole value, not
# merely its first character, so `'x` and `x'` are ordinary.
#
# Because there is one function, the special case is visible from all four
# directions at once — which is the point of checking them together rather than
# one at a time. `printf %q` is deliberately here too, as the control: it uses
# `sh_backslash_quote` instead and reaches `\'` by another road entirely.
#
# `declare -p` is *not* one of these: it quotes with double quotes, so a lone
# quote comes back as `"'"` and the special case never arises.
#
# Verified against bash 5.2.37.

p() { printf '%-46s -> [%s]\n' "$1" "$2"; }

q=\'

echo "=== the transforms ==="
p 'a lone quote, @Q'          "${q@Q}"
p 'a lone quote, @A'          "${q@A}"
p 'a quote then text'         "${qx@Q}"
qx=\'x;  p 'a leading quote'  "${qx@Q}"
xq=x\';  p 'a trailing quote' "${xq@Q}"
qq=\'\'; p 'two quotes'       "${qq@Q}"
e=;      p 'nothing at all'   "${e@Q}"
plain=hi; p 'a word needing nothing' "${plain@Q}"

echo "=== alias ==="
alias A=\'
alias A2=\'x
alias A3=x\'
alias A4=\'\'
alias A
alias A2
alias A3
alias A4

echo "=== trap -p ==="
trap \' EXIT
trap -p EXIT
trap - EXIT
trap \'x EXIT
trap -p EXIT
trap - EXIT

echo "=== declare -f re-quotes a word the same way ==="
# The parser has already translated each `$'…'` into a single-quoted run, using
# this same function — so what `declare -f` prints back is that translation.
f() { : $'\x27'; : $'\x27\x27'; : $'a\x27'; : $'\x27a'; }
declare -f f

echo "=== the controls ==="
p 'printf %q on a lone quote' "$(printf '%q' \')"
p 'printf %q on a leading one' "$(printf '%q' \'x)"
declare -p q
c=(\' "a'b")
declare -p c

echo done
