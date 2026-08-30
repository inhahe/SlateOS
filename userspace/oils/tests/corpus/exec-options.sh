# `exec` reads options of its own before the command word: `-a name` chooses the
# `argv[0]` the command will see, `-l` puts a `-` in front of that name, and `-c`
# hands the command an empty environment. Option parsing stops at `--`, at the
# first word that is not an option, and at a lone `-` — which is a command word
# rather than a terminator. A word `exec` cannot find gets its own wording: it
# names itself and says only "not found", where an ordinary command word would
# have said "command not found".
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== an option it does not know"
( exec -Z; echo "rc=$?" ) 2>&1 | e
( exec -Z zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec -cZ; echo "rc=$?" ) 2>&1 | e
( exec -a foo -Z; echo "rc=$?" ) 2>&1 | e

echo "=== …and one whose argument is missing"
( exec -a; echo "rc=$?" ) 2>&1 | e
( exec -ca; echo "rc=$?" ) 2>&1 | e

echo "=== options with nothing to run are just the redirections"
( exec -c; echo "rc=$?" ) 2>&1 | e
( exec -l; echo "rc=$?" ) 2>&1 | e
( exec -cl; echo "rc=$?" ) 2>&1 | e
( exec -a foo; echo "rc=$?" ) 2>&1 | e
( exec --; echo "rc=$?" ) 2>&1 | e
( exec -l >/dev/null; echo hidden ); echo "after=$?"

echo "=== a word it cannot find is exec's own wording"
( exec zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec -a foo zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec -c zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec -l zznosuchprog; echo "rc=$?" ) 2>&1 | e
( zznosuchprog; echo "rc=$?" ) 2>&1 | e

echo "=== the name may be attached to its letter, or come after it"
( exec -afoo zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec -ca foo zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec -la foo zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec -a foo -c zznosuchprog; echo "rc=$?" ) 2>&1 | e

echo "=== a lone - is a command word, and so is +c"
( exec -; echo "rc=$?" ) 2>&1 | e
( exec - zznosuchprog; echo "rc=$?" ) 2>&1 | e
( exec +c; echo "rc=$?" ) 2>&1 | e
( exec -- -Z; echo "rc=$?" ) 2>&1 | e

echo "=== …and after the command word the options are its own"
( exec zznosuchprog -c; echo "rc=$?" ) 2>&1 | e

echo "=== a command that does run, run under the options"
( exec -a foo echo hi ) 2>&1 | e
( exec -l echo hi ) 2>&1 | e
( exec -- echo hi ) 2>&1 | e

echo "=== done"
