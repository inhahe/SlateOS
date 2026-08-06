# A redirect list is not resolved and then installed; it is *performed*, one
# member at a time, against the real descriptor table. `do_redirections`
# (`redir.c`) walks the list and calls `do_redirection_internal` on each, and
# that function both expands the word and installs the result before the walk
# moves on. So member N's word is expanded with members 1..N-1 already in force,
# and anything the expansion writes to fd 2 — a `set -u` complaint, the stderr
# of a command substitution inside the word — goes wherever the list has already
# sent fd 2, not to the shell's own stderr.
#
# The second half of that is that a target is opened *once*, when its redirect
# is performed. Every later writer of the fd shares the one open file
# description, so they share its offset and none of them can truncate away what
# an earlier one wrote. That is why `2>err > $(echo boom >&2; …)` ends with both
# `boom` and the command's own output in `err`, and why `>f 2>&1` interleaves
# where `>f 2>f` — two independent opens, two offsets — clobbers.

echo '=== an earlier 2> catches what a later word says'
( true 2>err > $(echo boom >&2; echo /dev/null); printf '[%s]\n' "$(cat err)" )
( true > $(echo boom >&2; echo /dev/null) 2>err; printf '[%s]\n' "$(cat err)" )
( true 2>>err > $(echo boom >&2; echo /dev/null); printf '[%s]\n' "$(cat err)" )
( true 2>err 3> $(echo boom >&2; echo /dev/null); printf '[%s]\n' "$(cat err)" )

echo '=== including the here-document body, which is a word like any other'
( true 2>err <<EOF
$(echo boom >&2)
EOF
  printf '[%s]\n' "$(cat err)" )
( true 2>err <<< "$(echo boom >&2)"; printf '[%s]\n' "$(cat err)" )

echo '=== and a fatal expansion error, which is reported before it aborts'
# The complaint kills the shell performing the list, so the reading is done from
# outside it. `err` is removed first, because the reversed order never reaches
# its own `2>err` at all — the abort happens at the `$nope` before it — and a
# file left over from the line above would read as a hit.
rm -f err; ( set -u; true 2>err > $nope );         echo "rc=$?"; printf '[%s]\n' "$(cat err 2>/dev/null)"
rm -f err; ( set -u; true > $nope 2>err );         echo "rc=$?"; printf '[%s]\n' "$(cat err 2>/dev/null)"
rm -f err; ( set -u; true 2>err > ${nope:?boom} ); printf '[%s]\n' "$(cat err 2>/dev/null)"

echo '=== the redirect that then fails appends to the same file, never truncates it'
( true 2>err > $(echo boom >&2; echo /nodir/f); echo "rc=$?"; cat err )
( echo stale >err; true 2>err > $(echo boom >&2; echo /dev/null); printf '[%s]\n' "$(cat err)" )

echo '=== 2>&1 follows fd 1 as it stands at that moment, not as it will stand'
# The `>` has not been performed yet, so the `2>&1` copies the *ambient* stdout
# and `boom` reaches the terminal rather than the file.
( true 2>&1 > $(echo boom >&2; echo /dev/null) ) 2>&1
( true > $(echo boom >&2; echo /dev/null) 2>&1 ) 2>&1

echo '=== one open per redirect: shared offsets, and clobbering where they differ'
( { echo out; echo err >&2; } >f 2>&1;      printf '[%s]\n' "$(cat f)" )
( { echo out; echo err >&2; } &>f;          printf '[%s]\n' "$(cat f)" )
( { echo out; echo err >&2; } 2>f 1>&2;     printf '[%s]\n' "$(cat f)" )
( { echo out; echo err >&2; } >f 2>f;       printf '[%s]\n' "$(cat f)" )
( { echo a; echo b; echo c; } >f;           printf '[%s]\n' "$(cat f)" )
( echo one >f; echo two >f;                 printf '[%s]\n' "$(cat f)" )
( echo one >f; echo two >>f;                printf '[%s]\n' "$(cat f)" )
# One *builtin* writing both streams: its stdout and its diagnostics are
# installed by two different pieces of machinery, and they have to arrive at the
# one description all the same.
( type echo nosuchcmd >f 2>&1;              printf '[%s]\n' "$(cat f)" )
( type echo nosuchcmd 2>f 1>&2;             printf '[%s]\n' "$(cat f)" )
( type echo nosuchcmd &>f;                  printf '[%s]\n' "$(cat f)" )

echo '=== a `<>` still shares its own read half'
( printf 'ABCDEFGH' >f; exec 3<>f; read -r -n 3 -u 3 s; echo "read=$s"; echo XY >&3;
  exec 3>&-; printf '[%s]\n' "$(cat f)" )

echo '=== done'
