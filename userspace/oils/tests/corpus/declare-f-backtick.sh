# bash treats the two command-substitution spellings as different constructs
# when it prints a function back: a `$( … )` body is re-printed from the parse
# (so its spacing is normalised), while a `` ` … ` `` body is echoed from the
# source, verbatim. That is not just cosmetic — re-printing a backtick body
# drops the `\`` escapes a nested substitution needs, and the result no longer
# parses.
echo "=== \$( ) is re-printed, backticks are echoed verbatim"
f() {
	echo $(echo    a   ;   echo b)
	echo `if true;then echo c;fi`
	echo `echo a >/dev/null   2>&1`
	echo "`echo   q  `"
}
declare -f f

echo "=== nested and escaped forms survive"
g() {
	echo `echo \`echo nested\``
	echo `echo '$x'` `echo "$y"` `echo \$z`
	x=`echo a`"$(echo b)"`echo c`
	case `echo p` in `echo p`) echo m ;; esac
}
declare -f g

echo "=== a multi-line body keeps its own lines"
h() {
	echo `
echo multi
echo line
`
}
declare -f h

echo "=== the printed form runs the same and prints the same again"
y=Y
eval "$(declare -f g)"
g
declare -f g
eval "$(declare -f h)"
h

# A syntax error inside a `$( … )` body names the substitution's closing paren
# rather than the end of the body, because bash parses the body in the enclosing
# token stream. It is fatal to the script, so it has to come last.
echo "=== a syntax error in a \$( ) body names the closing paren"
echo $(if)
echo unreachable
