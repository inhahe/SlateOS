# The text a `$( … )` runs is not the text that was written. `parse_comsub`
# parses the body, keeps `print_comsub`'s re-print of the parse, and throws the
# parse away (parse.y:4219-4233):
#
#     tcmd = print_comsub (parsed_command);      /* returns static memory */
#     …
#     ret[retlen++] = ')';
#     …
#     dispose_command (parsed_command);
#
# `ret` is what the word carries, and `command_substitute` reads it back at
# expansion time. So the parse that found the `)` — and that raised any syntax
# error, which is why `$(for)` is fatal where `` `for` `` is silent — is not the
# parse that runs.
#
# That is unobservable while the re-print has the same *shape* as the source,
# which is why it can be mistaken for a numbering rule: for a body of simple
# commands the re-print drops blank lines, drops comments and joins nothing, so
# `$LINENO` looks like it counts "the rank of the body line among the lines that
# carry a command", starting at the line the `)` sits on.
#
# It is not a rank. It is the re-print's own line number, and a compound command
# gives the two different answers: `if true; then echo B; fi` occupies one source
# line and three re-printed ones, so a statement after it in the same body reads
# two lines lower than the source would suggest.
#
# A backtick body is the control: bash echoes it verbatim rather than re-printing
# it, so there the source's blank lines and continuation lines *are* counted.
echo "=== a compound command in the body pushes what follows it down"
r=$(if true; then echo B; fi
echo L=$LINENO)
echo "$r"

echo "=== nested one level deeper"
r=$(for i in a; do echo $i; done
echo L=$LINENO)
echo "$r"

echo "=== a plain body counts its own lines, blank ones dropped"
v=$(
echo $LINENO

echo $LINENO
)
echo "$v"

echo "=== comments are dropped too, and two commands on a line share a number"
w=$(
# a comment
echo $LINENO; echo $LINENO
echo $LINENO)
echo "$w"

echo "=== a backtick body is verbatim, so its blank lines do count"
q=`echo $LINENO

echo $LINENO`
echo "$q"

echo "=== and the enclosing script's own numbering is untouched"
echo $LINENO
