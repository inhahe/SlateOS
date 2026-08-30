# A `${…}` body has to *name* something. bash accepts four kinds of name — all
# digits, one of the seven single-character specials `@*#?-$!`, an array
# reference, or a legal identifier — and a body that is none of them is a bad
# substitution rather than a reference to a parameter with an odd name. The
# refusal is of the name alone, so it lands before any operator written after it
# is even considered, and — like every bad substitution — it waits until the
# word is actually expanded.
#
# Each probe runs in a subshell, because a bad substitution abandons the rest of
# the input it was read from.

echo "=== a body that cannot begin a name"
for b in . ' ' + = : / '^' , '~' % '<' '{' ']' ')' ';' '&' '|' '>' '('; do
  ( eval "echo \"\${$b}\"" ) 2>&1; echo "rc=$?"
done

echo "=== an empty body is the same verdict"
( echo "${}" ) 2>&1; echo "rc=$?"
( echo ${} ) 2>&1; echo "rc=$?"

echo "=== and it is reached only when the word is expanded"
( if false; then echo "${}"; fi; echo "guarded rc=$?" ); echo "rc=$?"
( if false; then echo "${.}"; fi; echo "guarded rc=$?" ); echo "rc=$?"
# Reached, it discards the rest of its parse unit but does not leave the shell.
( { echo "X${.}Y"; echo unreachable; }; echo also-unreachable ) 2>&1; echo "rc=$?"

echo "=== the operator written after the name never runs"
# The default is not substituted, the suffix does not strip, and the transform
# is not applied: bash rejects the name before it looks at what is asked of it.
for b in '.:-x' ':-x' '.#p' '.%p' '.^' '.@Q' '.:0:1' '.+x' '.:=y' './p/r'; do
  ( eval "echo \"\${$b}\"" ) 2>&1; echo "rc=$?"
done

echo "=== the seven specials do name themselves"
set -- p q
echo "[${#}] [${@}] [${*}] [${?}] [${!:-n}] [${@:-d}] [${-:+s}]"
# `$$` is a pid, so only its shape is checked.
case "${$}" in ''|*[!0-9]*) echo 'BAD $$';; *) echo 'ok $$';; esac

echo "=== so do a digit and an identifier"
v=z
echo "[${1}] [${12:-e}] [${v}] [${_v:-u}] [${0##*/}]"
# A multi-character body that starts badly was already refused, and still is.
( echo "${1a}" ) 2>&1; echo "rc=$?"
( echo "${ a}" ) 2>&1; echo "rc=$?"
( echo "${a }" ) 2>&1; echo "rc=$?"
