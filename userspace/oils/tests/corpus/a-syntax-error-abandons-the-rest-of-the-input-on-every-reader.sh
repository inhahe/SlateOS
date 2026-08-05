# `report_syntax_error` does not merely score the error. Unless the shell is
# interactive it finishes with `jump_to_top_level (FORCE_EOF)`, which sets
# `EOF_Reached` and so stops `reader_loop` outright — the rest of the input is
# never read, let alone run, and the shell exits 2.
#
# That is easy to miss for a script file or a `-c` string, because each is
# parsed as a single unit and the error already ends the only parse there was.
# Stdin is the reader that tells the two apart: it is read a *logical command*
# at a time, so a shell that merely scored the error would come back for the
# next line and run it.
#
#   echo one          one
#   [[ a              line 2: unexpected token `newline', conditional …
#   == b ]]           line 2: syntax error near `a'
#   echo tail         line 2: `[[ a'
#                     ↳ and that is all: no `== b ]]: command not found`,
#                       no `tail`, and the shell exits 2.
#
# The bar is only that high for the shell's *own* input. `eval` and `.`/`source`
# install their own top-level handler, so an error under them ends the string or
# the sourced file and the caller reads on.
#
# `$BASH` is the running shell either way, so each case below re-invokes it and
# strips the leading program name from the diagnostics, which naturally differ.

s() { sed 's/^.*: line /line /'; }

# The two streams are merged into a *pipe*, not into a `$( … )` capture: the
# whole point of these cases is which lines the shell got as far as writing, so
# they must arrive in write order, and `${PIPESTATUS[0]}` still yields the
# shell's own status.
o() { "$BASH" --norc "$@" 2>&1 | s; echo "rc=${PIPESTATUS[0]}"; }
i() { "$BASH" --norc < s.sh 2>&1 | s; echo "rc=${PIPESTATUS[0]}"; }

printf 'echo one\n[[ a\n== b ]]\necho tail\n' > s.sh

echo "=== a file stops at the error"
o s.sh

echo "=== so does a -c string"
o -c 'echo one
[[ a
== b ]]
echo tail'

echo "=== and so does stdin, which is read a command at a time"
i

echo "=== whatever the shape of the error"
for body in ';;' 'fi' ')' 'for' 'done' 'esac' '}' 'case x in ;;' 'echo a; ;;'; do
  printf -- '--- %s\n' "$body"
  printf 'echo one\n%s\necho tail\n' "$body" > s.sh
  i
done

echo "=== an unterminated construct ends the input anyway"
printf 'echo one\necho $(\necho tail\n' > s.sh
i
printf 'echo one\nif true\necho tail\n' > s.sh
i

echo "=== but a command merely spread over lines is no error at all"
printf 'echo one\ncase x in\nesac\necho tail\n' > s.sh
i
printf 'echo one\nif true; then\n  echo mid\nfi\necho tail\n' > s.sh
i
printf 'echo one &&\n  echo two\necho tail\n' > s.sh
i

echo "=== a runtime error is not a syntax error, and stops nothing"
printf 'echo one\nnosuchcmd\necho tail\n' > s.sh
i

echo "=== nor is one under eval or source, which end only themselves"
printf 'echo one\neval ";;"\necho tail\n' > s.sh
i
printf ';;\necho sourced-tail\n' > inner.sh
printf 'echo one\n. ./inner.sh\necho tail\n' > s.sh
i

echo "=== and the error's own status survives to be the shell's"
printf 'echo one\n;;\n' > s.sh
i
printf 'true\n;;\n' > s.sh
i
printf 'false\n;;\n' > s.sh
i

rm -f s.sh inner.sh
echo done
