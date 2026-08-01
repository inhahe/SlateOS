# `2>&-` closes fd 2, and a closed fd 2 is not an fd 2 that was never
# redirected — the first must drop what it is handed, the second must print it.
# The distinction is quieter than fd 1's, because a *failed* write to fd 2 has
# nowhere to be reported: the only visible effect of closing fd 2 is that the
# diagnostics stop, while the status stays exactly what it would have been.
# `cd /nosuchdir 2>&-` still exits 1.
#
# So the interesting question is not what fd 2 does with bytes but what other
# redirects can still do with fd 2:
#
#   * `1>&2` is a *dup*, performed when the redirect is set up, so duplicating a
#     closed fd 2 fails outright — status 1, nothing written, and the
#     `2: Bad file descriptor` itself invisible for the same reason everything
#     else is. The same holds for an fd 2 that exists but has no write half
#     (`2>&0` under a read-only fd 0), which is the same question asked of a
#     different way of having no writable description.
#   * `exec 3>&2` likewise cannot copy a descriptor that is not there.
#   * but a `>&2` taken *before* the close copies fd 2 while it is still open,
#     so `echo hi >&2 2>&-` writes to the original stderr and exits 0.
#
# And a close is not permanent: a later `2> file` in the same list, or a later
# `exec 2> file`, gives fd 2 back.
#
# Deliberately absent: the reverse order of that last pair — `2>&- >&2`, where
# bash fails the dup because the close came first. osh's redirect plan has no
# order of its own and answers both spellings the way the commoner
# `>&2 2>&-` one is answered. See TD-OILS14 and
# TD-OILS-TRANSIENT-CLOSE-OF-A-STD-FD-IS-A-NO-OP.
#
# Every persistent probe runs in a subshell so the close cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
printf 'seed\n' > in
exec 4>&2 2>err

echo "=== the diagnostic is dropped, the status is not"
( cd /nosuchdir 2>&- );                echo "  cd rc=$?"
( { cd /nosuchdir; } 2>&- );           echo "  group cd rc=$?"
( cd /nosuchdir 2<&- );                echo "  2<&- rc=$?"
( exec 2>&-; cd /nosuchdir );          echo "  exec cd rc=$?"
( exec 2<&-; cd /nosuchdir );          echo "  exec 2<&- rc=$?"

echo "=== an external command keeps its own status too"
( cat /nosuchfile 2>&- );              echo "  ext rc=$?"
( exec 2>&-; cat /nosuchfile );        echo "  exec ext rc=$?"

echo "=== duplicating a descriptor that is not there"
( { echo A >&2; } 2>&- );              echo "  group dup rc=$?"
( exec 2>&-; echo B >&2 );             echo "  exec dup rc=$?"
( exec 2>&-; exec 3>&2 );              echo "  3>&2 rc=$?"
( exec 2>&-; ls /nosuchdir >&2 );      echo "  ext dup rc=$?"
( { echo C >&2; } 2>&0 < in );         echo "  2>&0 rc=$?"

echo "=== but a dup taken before the close still holds"
( echo D >&2 2>&- );                   echo "  dup-first rc=$?"
( echo E >&2 2>e1 2>&- );              echo "  dup-first+reopen rc=$? e1=[$(cat e1)]"

echo "=== and a close can be undone"
( { echo F >&2; } 2>&- 2>e2 );         echo "  group reopen rc=$? e2=[$(cat e2)]"
( cd /nosuchdir 2>&- 2>e3 );           echo "  reopen rc=$? e3=[$(cat e3)]"
( exec 2>&-; exec 2>e4; cd /nosuchdir ); echo "  exec reopen rc=$? e4=[$(cat e4)]"

echo "=== a close after a file wins, as any later redirect does"
( cd /nosuchdir 2>e5 2>&- );           echo "  close-after rc=$? e5=[$(cat e5)]"

echo "=== fd 1 is untouched by any of it"
( echo G 2>&- );                       echo "  echo rc=$?"
( exec 2>&-; echo H );                 echo "  exec echo rc=$?"

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
