# The scratch-fd idiom — `3>&1` used to smuggle a stream past a redirect —
# when fd 1 is a *capture* rather than a file or a terminal.
#
# `$( … )` gives its body the write end of the collecting pipe as fd 1, so a
# `3>&1` taken inside one aliases fd 3 to that pipe. Everything written to fd 3
# is therefore collected by the substitution, and a `2>&3` in the same command
# is how the classic "capture stderr, discard stdout" idiom works at all.
#
# The same holds when fd 1 is a pipeline stage's pipe, and it holds for a
# persistent `exec 3>&1` as well as a scoped one — the descriptor outlives the
# `exec`, but not the substitution whose fd 1 it copied.
#
# A `&` job that inherited such a descriptor keeps it open exactly as it would
# keep a real pipe write end open, so the substitution waits for the job.

echo "=== a scoped 3>&1 inside a substitution reaches the capture"
r=$({ echo hi >&3; } 3>&1); echo "  [$r]"
r=$({ echo o; echo x >&3; } 3>&1); echo "  [$r]"
r=$({ echo o; echo e >&2; } 3>&1 2>&3); echo "  [$r]"
# The classic idiom: stderr to the capture, stdout to the void.
r=$({ echo o; echo e >&2; } 2>&1 >/dev/null); echo "  [$r]"
r=$( { echo a >&3; echo b >&3; } 3>&1 ); echo "  [$r]"

echo "=== ... and so does a persistent one"
r=$(exec 3>&1; echo via3 >&3); echo "  [$r]"
r=$(exec 3>&1 4>&1; echo v4 >&4; echo v3 >&3); echo "  [$r]"
r=$(exec 3>&1; exec 1>/dev/null; echo kept >&3; echo lost); echo "  [$r]"
# `exec 2>&3` after `exec 3>&1`: fd 2 lands in the capture too.
r=$(exec 3>&1; exec 2>&3; echo e >&2); echo "  [$r]"

echo "=== externals write through the aliased descriptor too"
r=$({ cat <<<'from cat' >&3; } 3>&1); echo "  [$r]"
r=$({ echo shell >&3; cat <<<'ext' >&3; } 3>&1); echo "  [$r]"
r=$({ "$BASH" -c 'echo E >&2; echo O'; } 3>&1 2>&3 >/dev/null); echo "  [$r]"

echo "=== a pipeline stage's fd 1 is its pipe"
{ echo p >&3; } 3>&1 | sed 's/^/  piped: /'
r=$( { echo q >&3; } 3>&1 | sed 's/^/got /' ); echo "  [$r]"

echo "=== a fd-3 alias taken outside the substitution is not the capture"
exec 3>t.txt
r=$(echo outer >&3); echo "  r=[$r] file=[$(cat t.txt)]"
exec 3>&-

echo "=== the descriptor still names a file when fd 1 is one"
{ echo f >&3; } 3>&1 >u.txt; echo "  u=[$(cat u.txt)]"

echo "=== a & job holds the capture through fd 3"
x=$( exec 3>&1; ( echo held >&3 & ) >/dev/null; sleep 0.3 ); echo "  [$x]"
y=$( { { sleep 0.1; echo bg >&3; } & } 3>&1 >/dev/null; sleep 0.3 ); echo "  [$y]"

echo "=== closing it"
r=$(exec 3>&1; exec 3>&-; echo gone >&3 2>/dev/null; echo st=$?); echo "  [$r]"
