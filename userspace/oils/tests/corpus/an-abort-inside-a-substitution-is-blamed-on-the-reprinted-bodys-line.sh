# A `$( … )` body is not re-read as it was written. At *parse* time bash throws
# the source text away and keeps a re-*print* of the parsed command instead:
#
#   tcmd = print_comsub (parsed_command);   /* returns static memory */
#                                             (parse.y:4219, in parse_comsub)
#   ret = make_command_string (command);      (print_cmd.c:170, print_comsub)
#
# and it is that printed text the later `parse_and_execute` reads. The printer
# breaks a compound command across lines the source never had, and the body's
# line numbers are counted in the printed text — so a diagnostic raised inside
# a one-line substitution can name a line past the end of the file.
#
# How far past depends only on where the *printer* puts the body, and the two
# loops disagree by one because bash prints them differently:
#
#   print_for_command:            print_until_or_while:
#     cprintf (";");                semicolon ();
#     newline ("do\n");             cprintf (" do\n");
#     (print_cmd.c:627)             (print_cmd.c:811, which still carries the
#                                    comment `/* was newline ("do\n"); */`)
#
#     for i in 1;                   while true; do
#     do                                <body>
#         <body>                    done
#     done
#
# so a `for` body lands on printed line 3 and a `while` body on printed line 2.
# Printed line 1 is the line the substitution *closes* on.
#
# The point of the pairs below is that the abort is not special: each section
# asks `$LINENO` from the same place the next one aborts from, and the two
# numbers are equal. Nothing about a discard, a usage error or a special
# builtin shifts the line — the re-print does it, and every command in the body
# sees the same shift. (known-issues TD-OILS-CMDSUB-ABORT-LINENO recorded this
# as an abort-only artifact of a "special-builtin usage error inside `$( )`";
# it is neither abort-only nor error-class-specific.)
#
# Nesting composes by *more* than the two shifts added together, because the
# inner substitution's printed text is embedded in the outer one: parsing the
# outer body walks the counter down to the outer print's last line before the
# inner body runs at all. `$(echo $(for … ))` therefore shifts 3 + 2, not 2.

echo "=== a plain body is not moved: line 1 of the print is the closing line"
x=$(echo "L $LINENO")
echo "[$x]"
y=$(shift a b; echo body)
echo "[$y] rc=$?"

echo "=== a for body is printed two lines below the line it closes on"
x=$(for i in 1; do echo "L $LINENO"; done)
echo "[$x]"
y=$(for i in 1; do break 1 2; done; echo body)
echo "[$y] rc=$?"

echo "=== a while body is printed one line below it"
x=$(while true; do echo "L $LINENO"; break; done)
echo "[$x]"
y=$(while true; do break 1 2; done; echo body)
echo "[$y] rc=$?"

echo "=== an until body prints like a while"
x=$(until false; do echo "L $LINENO"; break; done)
echo "[$x]"

echo "=== a backtick body is echoed verbatim, not re-printed, so nothing moves"
x=`for i in 1; do echo "L $LINENO"; done`
echo "[$x]"
y=`for i in 1; do break 1 2; done`
echo "[$y] rc=$?"

echo "=== the source's own line count is what body line 1 counts from"
x=$(for i in 1; do
  echo "L $LINENO"
done)
echo "[$x]"
y=$(for i in 1; do
  break 1 2
done)
echo "[$y] rc=$?"

echo "=== a nested substitution rides inside the outer print, so shifts compose"
x=$(echo $(for i in 1; do echo "L $LINENO"; done))
echo "[$x]"
y=$(echo $(for i in 1; do break 1 2; done))
echo "[$y] rc=$?"
x=$(echo $(while true; do echo "L $LINENO"; break; done))
echo "[$x]"
x=$(for i in 1; do echo $(echo "L $LINENO"); done)
echo "[$x]"

echo "=== and none of it reaches the caller: the fork keeps the counter home"
echo "after $LINENO"

echo "still here"
