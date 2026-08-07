# The `/` that ends the pattern of a `${var/pat/repl}` is the first one *at the
# body's own level*. A `/` inside a nested `$( … )`, `${ … }`, `` ` … ` ``,
# `$(( … ))` or a quoted run belongs to that construct, and ends nothing.
#
# bash never has to look: `parse_matched_pair` recorded where every nested
# construct ended while it read the body, so a `/` inside one was never a
# candidate for the separator. A parser that keeps the body as raw text has to
# recover that structure before it splits — otherwise the pattern is truncated
# mid-construct and the two halves no longer lex.
#
# Verified against bash 5.2.37.

s=zaaaz
sl='a/b'
q="za'bz"
p() { printf '%-38s -> [%s]\n' "$1" "$2"; }

echo "=== a / inside a construct is not the separator ==="
p 'command substitution' "${s/$(echo a/b)aaa/Y}"
p 'brace expansion'      "${s/${sl}aaa/Y}"
p 'backticks'            "${s/`echo a/b`aaa/Y}"
p 'arithmetic'           "${s/$((4/2))aaa/Y}"
p 'single quotes'        "${s/'a/b'aaa/Y}"
p 'double quotes'        "${s/"a/b"aaa/Y}"
p 'escaped slash'        "${s/a\/b/Y}"
# `$'…'` is not a single-quoted run — its `\'` is an ANSI-C escape, so the
# string runs past it and the `/` inside is still protected.
p 'ansi-c quotes'        "${s/$'a\/b'aaa/Y}"
p 'ansi-c escaped quote' "${q/$'a\'/b'aaa/Y}"
p 'locale quotes'        "${s/$"a/b"aaa/Y}"

echo "=== …and the pattern really did contain it ==="
t='za/bz'
p 'command substitution' "${t/$(echo a/b)/Y}"
p 'brace expansion'      "${t/${sl}/Y}"
p 'backticks'            "${t/`echo a/b`/Y}"
p 'single quotes'        "${t/'a/b'/Y}"
p 'double quotes'        "${t/"a/b"/Y}"
p 'escaped slash'        "${t/a\/b/Y}"
p 'ansi-c quotes'        "${t/$'a\/b'/Y}"
p 'ansi-c escaped quote' "${q/$'a\'b'/Y}"

echo "=== the same at every spelling of the operator ==="
u='a/b a/b'
p 'replace all'          "${u//$(echo a/b)/Y}"
p 'anchored at start'    "${u/#$(echo a/b)/Y}"
p 'anchored at end'      "${u/%$(echo a/b)/Y}"

echo "=== and on an array, element-wise ==="
arr=('za/bz' 'qa/bq')
p 'bulk replace'         "${arr[@]/$(echo a/b)/Y}"
p 'bulk replace all'     "${arr[*]//$(echo a/b)/Y}"

echo "=== a construct in the replacement half is untouched ==="
p 'sub in replacement'   "${t/a\/b/$(echo x/y)}"
p 'brace in replacement' "${t/a\/b/${sl}}"

echo "=== the replacement half keeps every escape for the later lex ==="
# Only `\/` in the *pattern* is this scan's own delimiter and consumed by it;
# everything else — including a `/` in the replacement, which nothing splits
# further — is passed on whole and interpreted where it belongs.
p 'escaped slash in repl' "${s/aaa/x\/y}"
p 'escaped slash in both' "${t/a\/b/x\/y}"
p 'bare slash in repl'    "${s/aaa/x//y}"
p 'quoted slash in repl'  "${s/aaa/'x/y'}"
p 'backslash in repl'     "${s/aaa/x\\y}"
p 'ampersand in repl'     "${s/aaa/[&]}"
p 'escaped ampersand'     "${s/aaa/[\&]}"
p 'slash is the repl'     "${s//a/\/}"
p 'ansi-c in repl'        "${s/aaa/$'x\'y'}"

echo "=== nesting inside the construct still closes correctly ==="
p 'quoted paren'         "${s/$(echo 'a/)b')aaa/Y}"
p 'nested substitution'  "${s/$(echo $(echo a/b))aaa/Y}"
p 'nested brace'         "${s/${x:-${sl}}aaa/Y}"

echo "=== an unterminated construct is still the later lex's error ==="
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }
e 'echo ${s/$(echo a/Y}'

echo done
