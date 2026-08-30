# `${PIPESTATUS[@]}` is written where the shell waits, not where it arranges.
#
# Every command sets `$?`, but not every command writes the array. The shell
# writes it when it waits for something — a simple command (a builtin, an
# external, a function call), a subshell, `[[ … ]]`, `(( … ))` — and a compound
# command that runs in the *current* shell is not that: `{ … }`, `if`, `for`,
# `while`, `case`, `select` are ways of arranging other commands, so they leave
# the array exactly as the last command *inside* them wrote it. If nothing
# inside them ran, the array is still whatever preceded the whole construct.
#
# The distinction is invisible as long as everything in sight is a one-command
# pipeline, since a compound's status usually equals the status of the last
# thing in it — so every reading below puts a *multi-stage* pipeline on one
# side of it. `false | false` leaves [1 1] and `true | true` leaves [0 0]; a
# one-element reading therefore means something wrote an array of its own.
#
# The reading is inside the `eval`, not after it, because `eval` is itself a
# simple command and would otherwise be the last thing to write the array.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"'; echo "ps=[${PIPESTATUS[*]}]"' ) 2>&1 | e; }

echo "=== the commands that do write one"
p 'false | false; true'
p 'false | false; [[ -n x ]]'
p 'false | false; (( 1 + 1 ))'
p 'false | false; ( true | true )'
p 'false | false; ( exit 7 )'
# A function call is a simple command, so the call writes one element and the
# pipeline inside the body does not survive it.
p 'f() { true | true; }; false | false; f'
p 'f() { false | false; }; true | true; f'

echo "=== …and the ones that leave the last command inside them standing"
p 'false | false; { true | true; }'
p 'false | false; { { true | true; }; }'
p 'false | false; if true; then true | true; fi'
p 'false | false; for i in a; do true | true; done'
p 'false | false; for (( i=0; i<1; i++ )); do true | true; done'
p 'false | false; case x in x) true | true;; esac'
p 'false | false; while :; do true | true; break; done'

echo "=== a construct whose insides never ran leaves the array untouched"
p 'true | true; if false; then :; fi'
p 'true | true; while false; do :; done'
p 'true | true; until :; do :; done'
p 'true | true; for i in ; do :; done'
p 'true | true; case x in y) :;; esac'
# A function *definition* is not a command that runs anything either.
p 'true | true; f() { :; }'

echo "=== redirections do not change the answer, even when they fail"
p 'false | false; ( true | true ) > /dev/null'
p 'false | false; { true | true; } > /dev/null'
p 'true | true; { echo x; } > /nonexistent/dir/f'
p 'true | true; ( echo x ) > /nonexistent/dir/f'

echo "=== nor does !, nor an and-or list, which only pick who runs"
# `!` inverts `$?` and leaves the array alone. `&&`/`||` write nothing of their
# own either — whichever side actually runs writes last, so the `:` shows up
# after `&&` (taken, the group succeeded) and not after `||` (not taken).
p 'false | false; ! { true | true; }'
p 'false | false; { true | true; } && :'
p 'false | false; { true | true; } || :'
# `time` reports on stderr, which is dropped here rather than merged: the
# report is the one thing in this file that cannot be compared literally.
t() { echo "--- $1"; ( eval "$1"'; echo "ps=[${PIPESTATUS[*]}]"' ) 2>/dev/null; }
t 'false | false; time { true | true; }'
t 'false | false; time ( true | true )'
