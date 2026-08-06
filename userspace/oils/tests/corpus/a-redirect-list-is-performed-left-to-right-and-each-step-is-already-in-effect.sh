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

echo '=== every descriptor the list has bound, not just fd 2'
# fd >= 3 is bound by the step that names it and is gone again once the list
# ends, so a later word can write to it and the command after cannot.
( true 3>f3 > $(echo x >&3; echo /dev/null);       printf '[%s]\n' "$(cat f3)" )
( true 3>f3 4>&3 > $(echo y >&4; echo /dev/null);  printf '[%s]\n' "$(cat f3)" )
( true > $(echo x >&3; echo /dev/null) 3>f3;       printf '[%s]\n' "$(cat f3)" ) 2>&1
( true 3>f3 3>f4 > $(echo z >&3; echo /dev/null);  printf '[%s][%s]\n' "$(cat f3)" "$(cat f4)" )
( true 3>f3 3>&- > $(echo q >&3; echo /dev/null);  printf '[%s]\n' "$(cat f3)" ) 2>&1
( true 3>f3 > /dev/null; echo after >&3 )          2>&1
# The binding the list started with is what comes back, even though the same
# descriptor was bound twice inside it, and the two share one offset.
( exec 3>outer; true 3>f3 > $(echo x >&3; echo /dev/null); echo back >&3; exec 3>&-
  printf '[%s][%s]\n' "$(cat f3)" "$(cat outer)" )
( exec 3>outer; true > $(echo x >&3; echo /dev/null) 3>f3; echo back >&3; exec 3>&-
  printf '[%s]\n' "$(cat outer)" )
# A failure part-way through still leaves what the earlier steps wrote — fd 3
# was bound and written before the `>` failed.
( true 3>f3 > $(echo w >&3; echo /nodir/f);        printf '[%s]\n' "$(cat f3)" ) 2>&1
# A fatal expansion error in an *argument* aborts before the redirect list is
# walked at all, so `echo v >&3` never runs. The abort kills the subshell, so
# whether the list got far enough to create `f3` is asked from outside it.
rm -f f3
( set -u; true 3>f3 > $(echo v >&3; echo ok) $nope; printf 'unreached\n' ) 2>&1
if [ -e f3 ]; then printf 'f3 exists [%s]\n' "$(cat f3)"; else echo 'f3 missing'; fi

echo '=== and fd 0, whose reader shares the one offset with the command'
printf 'L1\nL2\nL3\n' > in
( true <in 3> $(cat >&2; echo /dev/null) ) 2>&1
( true <in 3> $(read -r l; echo "l=[$l]" >&2; echo /dev/null) ) 2>&1
# Two words in the same list drain successive lines, and the command picks up
# where the last word left off — one descriptor, one offset, as bash has it.
( true <in 3> $(read -r a; echo "a=[$a]" >&2; echo /dev/null) \
             4> $(read -r b; echo "b=[$b]" >&2; echo /dev/null) ) 2>&1
( { read -r c; echo "c=[$c]"; } <in 3> $(read -r a; echo "a=[$a]" >&2; echo /dev/null) ) 2>&1
# …and the reverse order reads the ambient fd 0, not the file.
( true 3> $(read -r a; echo "a=[$a]" >&2; echo /dev/null) <in ) 2>&1
( true <&- 3> $(read -r a; echo "rc=$?" >&2; echo /dev/null) ) 2>&1
( true <in 3>/dev/null; read -r z; echo "z=[$z]" ) 2>&1

echo '=== including the here-document body, which is a word like any other'
( true 2>err <<EOF
$(echo boom >&2)
EOF
  printf '[%s]\n' "$(cat err)" )
( true 2>err <<< "$(echo boom >&2)"; printf '[%s]\n' "$(cat err)" )
# A here-document is also a *source* like any other once it is performed, so a
# word after it reads it — and a word before it does not.
( true <<< $'H1\nH2' 3> $(read -r a; echo "a=[$a]" >&2; echo /dev/null) ) 2>&1
( true 3> $(read -r a; echo "a=[$a]" >&2; echo /dev/null) <<< $'H1\nH2' ) 2>&1

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
