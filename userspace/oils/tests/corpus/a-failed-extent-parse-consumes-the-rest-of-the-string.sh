# `extract_command_subst` finds a `$( … )`'s extent by running a real parse over
# everything after the `$(` — `xparse_dolparen` (parse.y:4248). When that parse
# *fails* the read still has to say where it stopped, and for every shape here it
# stopped at the end of the string: `parse_string`'s `SEVAL_NOLONGJMP` branch does
# `reset_parser (); break;` (builtins/evalstring.c:688-695) with the string reader
# having already swallowed the whole text — which for a one-line body is one
# "line". A body with a newline in it stops earlier, on the line the reader was
# holding; that is the general rule, and
# `a-failed-extent-read-stops-on-the-line-the-reader-reached.sh` is it.
# Two things follow, from parse.y:4338-4377:
#
#     if (ep[-1] != ')')
#       { while (ep > ostring && ep[-1] == '\n') ep--; }
#     nc = ep - ostring;
#     *indp = ep - base - 1;
#     ret = (nc == 0) ? "" : substring (ostring, 0, nc - 1);
#
#   * the text handed to `command_substitute` is the **whole remainder less one
#     byte**. The `nc - 1` is there to drop the `)` a successful read ends on; a
#     failed one has no `)` there, so it costs a real character.
#   * `*indp` lands on the last byte, so `param_expand` returns with the caller's
#     index past everything: the rest of the string is **consumed**, literals and
#     further expansions alike.
#
# So each failure is reported twice with *different* text: the extent read echoes
# the whole remainder, and the child that then runs it echoes one byte less.
#
# Only reachable under `no_longjmp_on_fatal_error` — a prompt expansion (`PS1`,
# `PS4`, `${x@P}`) — because otherwise the read's `jump_to_top_level (DISCARD)`
# abandons the command before any of it matters.
#
# Verified against bash 5.2.37.

echo "=== the body run is the remainder less its last byte"
u='$(fi)tail';          printf '1 [%s]\n' "${u@P}"
v='pre $(fi) post';     printf '2 [%s]\n' "${v@P}"
w='$(fi)';              printf '3 [%s]\n' "${w@P}"
x='a$(echo hi)b$(fi)c'; printf '4 [%s]\n' "${x@P}"

echo "=== and everything after it is consumed, not expanded"
y=Y
b='A$(fi)B${y}C';        printf '5 [%s]\n' "${b@P}"
g='A$(fi)B$(echo hi)C';  printf '6 [%s]\n' "${g@P}"
# The consumption is the *string* level's, so a second substitution after the
# failed one never runs — its text is inside the body that did.
h='A$(fi)B"q"C';         printf '7 [%s]\n' "${h@P}"

echo "=== a backquote is a different extractor and consumes nothing"
e='A`fi`B';              printf '8 [%s]\n' "${e@P}"

echo "=== an unclosed \$( is the same rule with no ) to drop"
t='a$(echo';             printf '9 [%s]\n' "${t@P}"
s='a$(echo hi';          printf '10 [%s]\n' "${s@P}"

echo "=== the status is the prompt's, which is to say untouched"
false; z='$(fi)q'; printf '11 [%s] rc=%s\n' "${z@P}" "$?"

echo "=== PS4 reaches it by the same route"
( PS4='$(fi)+ '; set -x; : m; set +x ) 2>&1
echo "prompt rc=$?"

echo "=== off the prompt path the read jumps instead"
( eval 'cat <<E
$(fi)tail
E' ) 2>&1
echo "after=$?"

echo done
