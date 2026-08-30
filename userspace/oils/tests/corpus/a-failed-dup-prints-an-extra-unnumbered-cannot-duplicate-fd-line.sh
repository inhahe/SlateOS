# A dup whose source is not open reports it twice. Ahead of the familiar
# numbered `<n>: Bad file descriptor` bash may print a second, *unnumbered*
# line —
#
#     redirection error: cannot duplicate fd: Bad file descriptor
#
# — through `sys_error`, so with the shell's name and no `line N:` token. It
# comes from an `fcntl(F_DUPFD)` on the dup's *source*, of which `redir.c` has
# two along the dup path, and each reports its own failure:
#
#   * `{v}>&N` / `{v}<&N` dups the source to allocate the variable's own
#     descriptor. That save is how the redirect is *performed*, so it is not
#     tied to keeping an undo list: a forked child prints it too.
#   * `N>&M-` saves `M` so the close a move adds can be undone. That one is
#     guarded twice — the redirector must already be open, and the shell must
#     be keeping an undo list at all — so a command bash performs in a child
#     stays silent. `exec` is not such a case: its list is a builtin's, applied
#     with `RX_ACTIVE|RX_UNDOABLE` like any other, and `exec` merely throws the
#     undo list away afterwards.
#
# Both saves are of the source, so both fail exactly when the source is not
# open — which is when the numbered line is raised. The extra line therefore
# never appears on its own, never accompanies `ambiguous redirect`, and never
# attaches to a plain `N>&M`.

echo '=== a move performed in the shell, onto a redirector that is open'
( true 1>&9-; echo "rc=$?" )
( true <&9-; echo "rc=$?" )
( true 0<&9-; echo "rc=$?" )
( exec 5>&1; true 5>&9-; echo "rc=$?" ) 2>&1

echo '=== and so does every compound the shell performs itself'
( { true; } 1>&9-; echo "rc=$?" )
( while :; do break; done 1>&9-; echo "rc=$?" )
( if :; then :; fi 1>&9-; echo "rc=$?" )
f() { :; }; ( f 1>&9-; echo "rc=$?" )

echo '=== but a forked one has no undo list to build, so it stays silent'
( /bin/true 1>&9-; echo "rc=$?" )
( /bin/true 3>&9-; echo "rc=$?" )
( ( : ) 1>&9-; echo "rc=$?" )
( ( : ) 3>&9-; echo "rc=$?" )

echo '=== a null command follows the same fork rule it always does'
( 1>&9-; echo "rc=$?" )
( 3>&9-; echo "rc=$?" )
( <&9-; echo "rc=$?" )

echo '=== `exec` is performed in the shell like any other builtin'
( exec 1>&9-; echo "rc=$?" ) 2>&1
( exec 3>&9-; echo "rc=$?" ) 2>&1
( exec 5>&1; exec 5>&9-; echo "rc=$?" ) 2>&1

echo '=== the redirector must already be open for there to be a save'
( true 3>&9-; echo "rc=$?" )
( true 5>&9-; echo "rc=$?" )
( true 9>&8-; echo "rc=$?" )
( exec 9>&8-; echo "rc=$?" ) 2>&1

echo '=== a varfd dup reports it whoever performs it, and move or not'
( true {v}>&9; echo "rc=$?" )
( true {v}<&9; echo "rc=$?" )
( true {v}>&9-; echo "rc=$?" )
( /bin/true {v}>&9; echo "rc=$?" )
( /bin/true {v}>&9-; echo "rc=$?" )
( exec {v}>&9; echo "rc=$?" ) 2>&1
( {v}>&9; echo "rc=$?" )

echo '=== never for a failure that is not a bad descriptor'
( true {v}>&nope; echo "rc=$?" )
( true 3>&nope-; echo "rc=$?" )
( v=; true 3>&$v-; echo "rc=$?" )
( true 1>&-; echo "rc=$?" )
( true 3>&-; echo "rc=$?" )

echo '=== and never for a plain dup'
( true 1>&9; echo "rc=$?" )
( true 3>&9; echo "rc=$?" )
( /bin/true 3>&9; echo "rc=$?" )
( exec 3>&9; echo "rc=$?" ) 2>&1

echo '=== done'
