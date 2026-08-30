# Which backslashes a `` ` … ` `` body loses before it is parsed.
#
# The escapes belong to the *enclosing* text, not to the command inside, so
# they are removed before the body is ever read as a command. Everywhere,
# that is `` \` ``, `\\` and `\$`; inside a double-quoted string it is also
# `\"`, because there the backslash is one of the ones double quotes give
# meaning to. So a `\"` in a backtick body is gone even when the body would
# have quoted the character, and the very same body outside double quotes
# keeps it.
#
# `$( … )` is a different construct: its body is parsed as ordinary source, so
# none of this applies to it.

echo "=== \\\" is stripped inside double quotes, kept outside"
echo "`echo \"x\"`"
echo `echo \"x\"`
v="`echo \"y\"`"; echo "[$v]"
w=`echo \"y\"`;   echo "[$w]"
echo "abc`echo \"q\"`def"
echo "`echo '\"'`"
echo "`echo \"a b\"`"

echo "=== the escapes stripped everywhere"
echo "`echo \\\$HOME`"
echo `echo \\\$HOME`
echo "`echo \`echo inner\``"
echo "`echo a\\\\b`"

echo "=== \$( ) strips none of them"
echo "$(echo \"x\")"
echo "$(echo '\"')"

echo "=== a here-doc body is a quoted context but not a quoted string"
cat <<EOF
`echo \"x\"`
EOF
cat <<"EOF"
`echo \"x\"`
EOF

echo "=== and the source spelling survives for declare -f"
f() { echo "`echo \"x\"`" `echo \"y\"`; }
declare -f f
f
eval "$(declare -f f)"
f
