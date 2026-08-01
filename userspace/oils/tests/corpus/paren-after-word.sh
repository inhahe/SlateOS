# A `(` after a word opens a function definition — `WORD '(' ')' body` — and
# nothing else, so once the parser has read it the `)` is compulsory and the
# error names whatever stands in its place, never the `(` that was accepted.
# `((` is an arithmetic command only where a reserved word would be recognised;
# after an ordinary word or an assignment prefix it is just two `(` tokens, and
# so lands in the same function-definition production.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== the token found where the ) was wanted"
( eval 'a ( b'; echo "rc=$?" ) 2>&1 | e
( eval 'a (b)'; echo "rc=$?" ) 2>&1 | e
( eval 'a ( ;'; echo "rc=$?" ) 2>&1 | e
( eval 'a ('; echo "rc=$?" ) 2>&1 | e
( eval 'echo ('; echo "rc=$?" ) 2>&1 | e
( eval 'a ( | b'; echo "rc=$?" ) 2>&1 | e
( eval 'a ( ) '; echo "rc=$?" ) 2>&1 | e

echo "=== …and an assignment word is not part of the production"
( eval 'a=b ( c'; echo "rc=$?" ) 2>&1 | e
( eval 'a=b ()'; echo "rc=$?" ) 2>&1 | e

echo "=== (( where a command cannot start is two parens"
( eval 'a (('; echo "rc=$?" ) 2>&1 | e
( eval 'echo ((1))'; echo "rc=$?" ) 2>&1 | e
( eval 'echo ((1'; echo "rc=$?" ) 2>&1 | e
( eval 'a b ((c'; echo "rc=$?" ) 2>&1 | e
( eval 'x=1 ((2))'; echo "rc=$?" ) 2>&1 | e
( eval 'case x in ((x) echo hit;; esac'; echo "rc=$?" ) 2>&1 | e

echo "=== …and where it can, it is arithmetic"
( eval '((1)); echo ok=$?' ) 2>&1 | e
( eval 'if ((1)); then echo yes; fi' ) 2>&1 | e
( eval 'while ((0)); do echo no; done; echo ok=$?' ) 2>&1 | e
( eval 'true && ((1)) && echo and-ok' ) 2>&1 | e
( eval 'true | ((1)); echo pipe-ok=$?' ) 2>&1 | e
( eval 'true; ((1)); echo semi-ok=$?' ) 2>&1 | e
( eval '{ ((1)); }; echo brace-ok=$?' ) 2>&1 | e
( eval '( ((1)) ); echo sub-ok=$?' ) 2>&1 | e
( eval '! ((0)); echo bang-ok=$?' ) 2>&1 | e
( eval 'for ((i=0;i<2;i++)); do echo i=$i; done' ) 2>&1 | e
( eval 'for i in 1; do ((i)); done; echo do-ok=$?' ) 2>&1 | e
( eval 'case x in x) ((1));; esac; echo case-ok=$?' ) 2>&1 | e

echo "=== nested subshells still want the space"
( eval '( (echo hi) )' ) 2>&1 | e
( eval 'f() ( echo body ); f' ) 2>&1 | e

echo "=== a function definition still parses every way it did"
( eval 'a ( ) { echo one; }; a' ) 2>&1 | e
( eval 'my-func() { echo two; }; my-func' ) 2>&1 | e
( eval 'function g() { echo three; }; g' ) 2>&1 | e

echo "=== done"
