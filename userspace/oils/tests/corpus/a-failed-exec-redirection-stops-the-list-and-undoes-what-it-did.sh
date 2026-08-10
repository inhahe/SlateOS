# `exec`'s redirections are permanent, but only if the whole list succeeds.
# `do_redirections` returns at the *first* failure —
#
#     for (temp = list; temp; temp = temp->next)
#       {
#         error = do_redirection_internal (temp, flags, &fn);
#         if (error)
#           { redirection_error (temp, error, fn); FREE (fn); return (error); }
#       }                                              /* redir.c:260-271 */
#
# — so the redirects after it are never performed, and `execute_builtin` then
# runs the undo list that `add_undo_redirect` / `add_undo_close_redirect` built
# as it went:
#
#     if (do_redirections (redirects, RX_ACTIVE|RX_UNDOABLE) != 0)
#       { undo_partial_redirects (); dispose_exec_redirects (); … }
#                                                 /* execute_cmd.c:5457-5465 */
#
# The undo is a *restore*, not a close: `add_undo_redirect` dups the descriptor
# aside before the redirect overwrites it, so a descriptor that was already
# bound goes back to what it was bound to, one that was not is closed, and one
# the list *closed* is opened again.
#
# Two things survive it.
#
#   * A `{name}` allocation. bash saves the redirector it just assigned and so
#     restores it to itself — `if (fd != redirector && (rflags & REDIR_VARASSIGN)
#     && varassign_redir_autoclose) add_undo_close_redirect (redirector); else if
#     ((fd != redirector) && fcntl (redirector, F_GETFD, 0) != -1)
#     add_undo_redirect (redirector, ri, -1);` (redir.c:959-962). Only
#     `varredir_close` takes the first branch and closes it. Either way `$name`
#     keeps the number.
#   * Everything a *fatal expansion* error left behind. That is not a
#     redirection failure at all: it unwinds rather than returning an error
#     code, so the undo list is discarded unrun.
#
# Verified against bash 5.2.37.

echo "=== the list stops at the first failure ==="
exec 4>e1 3>&9 5>e2
echo "1 rc=$?"
echo "1 e1=[$([ -e e1 ] && echo yes)] e2=[$([ -e e2 ] && echo yes)]"
echo four >&4 2>&1
echo "2 rc=$?"

echo "=== a descriptor that was already bound goes back to what it held ==="
exec 3>a1
exec 3>a2 4>&9
echo "3 rc=$?"
echo THREE >&3
echo "4 rc=$? a2=[$([ -e a2 ] && echo yes)]"

echo "=== …and one the list closed is open again ==="
exec 6>b1
exec 6>&- 7>&9
echo "5 rc=$?"
echo SIX >&6
echo "6 rc=$?"

echo "=== the standard descriptors are restored too ==="
exec >o1 2>&8
echo "7 rc=$?"
echo "8 still on the terminal"

echo "=== a {name} allocation survives, unless varredir_close ==="
exec {v}>v1 3>&9
echo "9 rc=$? v_set=$([ -n "$v" ] && echo yes)"
echo VEE >&$v
echo "10 rc=$?"
( shopt -s varredir_close
  exec {w}>w1 3>&9
  echo "11 rc=$? w_set=$([ -n "$w" ] && echo yes)"
  echo DOUBLEU >&$w
  echo "12 rc=$?" ) 2>&1

echo "=== a fatal expansion error is not that path, and undoes nothing ==="
unset z
exec 8>c1 9>"/dev/null${z:-'$(fi)'}" 4>c2
echo "13 rc=$? c1=[$([ -e c1 ] && echo yes)] c2=[$([ -e c2 ] && echo yes)]"
echo EIGHT >&8
echo "14 rc=$?"

exec 3>&- 4>&- 6>&- 8>&-
echo "=== what each file ended up holding ==="
echo "a1=[$(cat a1 2>&1)]"
echo "a2=[$(cat a2 2>&1)]"
echo "b1=[$(cat b1 2>&1)]"
echo "o1=[$(cat o1 2>&1)]"
echo "v1=[$(cat v1 2>&1)]"
echo "c1=[$(cat c1 2>&1)]"
echo TAIL
