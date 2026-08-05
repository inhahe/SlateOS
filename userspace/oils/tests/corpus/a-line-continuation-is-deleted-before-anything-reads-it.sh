# bash deletes a `\<newline>` in its *reader*: `shell_getc` throws the pair away
# before any scanner is handed a character of it. So the deletion is not a
# word-level escape that only a word can carry — it can stand anywhere two
# characters merely have to be adjacent, and the two sides simply meet:
#
#   false |\<newline>| echo x    is the operator `||`, not `|` then `|`
#   echo $\<newline>(echo hi)    is `$(`
#   echo ${v\<newline>x}         names the variable `vx`
#   echo x 2\<newline>>&1        is the IO number 2 attached to a `>&`
#   echo $((1 +\<newline>1))     is arithmetic, whose text never holds a `\`
#
# It reaches even the places a word scanner never runs: between `<` and `(` of a
# process substitution, between `{fd}` and the `>` that assigns it, and between
# the two `)` that close `$(( ))`.
#
# The places the reader stops removing them are the ones where a backslash is
# not an escape to begin with: inside `'…'`, inside a here-document whose
# delimiter was quoted, and inside a comment. There the backslash and the
# newline are ordinary text — so a comment still ends at its newline, and the
# next line runs.
#
# Because the pair is gone before the parser counts anything, it never costs a
# line: the newline still advanced the reader, so `$LINENO` on the line after a
# continuation is that line's own number. The line-number cases source a written
# file rather than `eval`, since `eval` reports the *caller's* line and would
# pin this file's own numbering.

echo "=== between the two characters of an operator"
false |\
| echo pipe-or
true &\
& echo and-and
case x in x) echo semi-semi ;\
; esac
echo redir >\
> co1; cat co1; rm -f co1
cat <\
<EOF
heredoc
EOF
cat <<\
< herestring
( exec 9<\
> co2 && echo readwrite; exec 9>&-; rm -f co2 )
echo to-stderr >\
&2
( set -C; echo noclobber >\
| co3 && cat co3; rm -f co3 )
echo pipe-and |\
& cat
case x in x) echo fallthrough ;\
& *) echo landed;; esac
(\
( 1 + 1 )) && echo arith-command
cat <\
(echo procsub)

echo "=== between a dollar and what it introduces"
echo $\
(echo cmdsub)
v=V; echo $\
{v}
echo $\
v
echo $\
((1+1))
echo ${v\
}
echo ${u:\
-default}
w=abc; echo ${#\
w}
vx=W; echo $v\
x
echo ${v\
x}
echo ${w#\
a}

echo "=== inside arithmetic, which keeps no backslash at all"
echo $((1 +\
1))
echo $((1 <\
< 2))
echo $((1+1)\
)

echo "=== other places two characters have to be adjacent"
echo io-number 2\
>&1
( exec 1\
2>/dev/null; echo io-split )
( exec {fd}\
>co4 && echo varfd; rm -f co4 )
\
echo leading-continuation
i\
f true; then echo reserved-word; fi
[\
[ a == a ]] && echo double-bracket
a\
=assigned; echo $a
echo $(false |\
| echo cmdsub-operator)
echo `echo back\
tick`
case ab in a\
b) echo case-pattern;; esac
case b in a|\
b) echo case-bar;; *) echo missed;; esac
for x in\
 1; do echo for-in $x; done
echo comment # a comment keeps it, and still ends here \
echo the-next-line-runs

echo "=== a quoted span keeps both characters"
printf '%s|' 'a\
b'; echo
printf '%s|' "a\
b"; echo
cat <<EOF
unquoted-a\
b
EOF
cat <<'EOF'
quoted-a\
b
EOF
echo ${u:-'a\
b'}
cat <<< 'herestring-a\
b'

echo "=== it costs no line"
l() { printf '%s\n' "$1" > sourced.sh; ( . ./sourced.sh ) 2>&1; rm -f sourced.sh; }
l '\
echo $LINENO'
l 'echo \
$LINENO'
l 'false |\
| echo $LINENO'
l '\
\
echo $LINENO'
l '  \
   echo $LINENO'
l 'echo one
echo \
$LINENO'

echo "=== and the operator it completed is the one that is parsed"
l 'echo a;\
; b'
l 'echo a |\
|| b'
l 'echo x >\
>'
echo done
