# The `in …` word list of a `for`/`select` loop is a plain word list: the lexer
# only promotes a token to a reserved word in *command* position, and this is
# not one. Measured against bash 5.2.
#
#   * every reserved word — `do` and `done` included — is an ordinary word
#     there, so `for x in if then fi` iterates over three literals;
#   * consequently the list is terminated only by something that is not a word
#     at all (`;`, a newline, `|`, `&`, `)`, end of input), which is why
#     `for x in a b do …` with no separator is a *syntax error*: `do` was
#     swallowed by the list, so the error surfaces later, at `done`;
#   * `in` itself is only the keyword in the first position after the loop
#     variable; a second `in` is a word;
#   * no `in` at all iterates `"$@"`, which is distinct from an empty list.
#
# The syntax-error lines name the shell — `$0`, which under `-c` is the path the
# shell was invoked as (TD-OILS-DOLLAR-ZERO-ARGV0) and so differs per host. They
# are folded to stdout with that leading "<shell>: " stripped. The pattern must
# not be `[^:]*`: a Windows path carries a drive-letter colon of its own.
sq() { sed 's/^.*: -c: /SH: -c: /' "$1"; }
bad() { "$BASH" -c "$1" >o.txt 2>&1; echo "rc=$?"; sq o.txt; }

echo "=== reserved words are ordinary words in the list"
for x in if then elif else fi while until do done; do printf '[%s]' "$x"; done; echo
for x in case esac in function time coproc select '{' '}' '!'; do printf '[%s]' "$x"; done; echo
for x in in in in; do printf '[%s]' "$x"; done; echo

echo "=== select offers them too"
printf '2\n' | COLUMNS=80 "$BASH" -c 'select o in a fi b; do echo "got=$o"; break; done' 2>&1

echo "=== so a missing separator before do is an error, reported at done"
bad 'for x in a b do echo hi; done'
bad 'select o in a b do echo hi; done'
bad 'for x in a b'

echo "=== a newline separates the list just as well as ;"
for x in a b
do printf '[%s]' "$x"; done; echo

echo "=== an operator ends the list where a word cannot follow"
bad 'for x in a b | c; do echo hi; done'
bad 'for x in a b & do echo hi; done'

echo "=== no in, an empty in, and a quoted reserved word"
set -- p q
for x; do printf '[%s]' "$x"; done; echo "  (no in)"
for x in; do printf '[%s]' "$x"; done; echo "  (empty)"
for x in \do 'done' "fi"; do printf '[%s]' "$x"; done; echo
