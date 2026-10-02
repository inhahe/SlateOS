## B-CP-SYMLINK-OWNERSHIP-FAILURE-QUOTED-A-NAME-GNU-LEAVES-BARE — FIXED 2026-09-01

**In short:** when `cp -P -p` could not give a copied symbolic link (a file that
is just a stored path pointing at another file) the same owner as the original,
it named the link in quotes — `failed to preserve ownership for './out'` — where
GNU names it bare: `failed to preserve ownership for ./out`. Same error, same
exit status, one character pair different. It survived because *no test case in
either harness could make that sentence print at all*.

**Where.** `userspace/coreutils/src/copy.rs`, the ownership-failure diagnostic,
which had one form for all three kinds of thing being copied.

**What GNU does, and why it is not an inconsistency on its side.** Upstream
quotes the name for a regular file and for a directory, and leaves it bare for a
symlink, because a symlink's ownership is not taken by the same code. Files and
directories go through `set_owner`, which writes `quoteaf (dst_name)`
(`copy.c:960`). A symlink's is a separate `lchownat` inlined into
`copy_internal` (`copy.c:3180`), and whoever wrote it used a plain `%s`. Ours
had `set_owner`'s form everywhere, so it was uniform where upstream is not.

**Why no test could reach it.** The obvious route is self-cancelling. Make a
chown fail the ordinary way — an unprivileged `cp -p` of a file owned by someone
else — and it fails with `EPERM`; but `chown_failure_ok` forgives `EPERM` and
`EINVAL` *unless the caller has chown privileges*. So the arrangement that makes
the chown fail is the same one that makes the program say nothing about it. Both
sides exit 0 in silence and agree, and a divergence in a sentence neither side
prints is invisible.

**Fixed** in `dfdbaefcf`, keyed on `made`, which already distinguishes the two
cases for the same underlying reason. The bare form is `escape_os`, not
`Path::display`: `display` substitutes U+FFFD for every byte that is not valid
UTF-8, so a link named in Latin-1 would be reported under a name that is not its
own, and the reader's obvious next move — pasting it into a shell — would find
nothing. `escape_os` is gnulib's `escape` style, identical to `%s` for every
name that is text.

**And then made reachable**, in `560a85c23`, because a fix found by reading
`copy.c` is not a way of finding the next one. A user namespace breaks the
deadlock and needs both halves: inside `unshare -Ur` the euid is 0, so the
forgiveness is withdrawn, and only one uid is mapped, so a source owned by
anyone else has an unmapped owner and the lchown fails `EINVAL` rather than
`EPERM`. Discovery is in `diff-wsl.sh` and probes by doing the thing, requiring
the lchown to fail `EINVAL` *specifically* — an `EPERM` would mean the
arrangement had collapsed back to asserting silence against silence. Verified in
both directions: with the fix, 581 passed / 0 differed / 30 differ on purpose;
with the fix reverted, 579 / 2 / 30, and the two are these, reporting `'out'`
against `out`.

**Only `cp` can reach it.** Every `cp` option that turns ownership on also sets
`require_preserve` (`cp.c:1018`, `1089`, `1098`), so the error is always fatal
there. `mv` leaves it false (`mv.c:143`) and would exercise the surviving
branch, but an `mv` source has to be one we can delete, and a foreign-owned link
inside a directory we own cannot be created without the privilege whose absence
is the whole point.

**A correction to `dfdbaefcf`'s own commit message**, recorded here because that
commit is pushed and cannot be amended: it cites `copy.c:961` for `set_owner`'s
`quoteaf (dst_name)`. The call is at **960**; 961 is the `if
(x->require_preserve)` below it. Its other citation, `copy.c:3186` for "a
separate `lchownat` inlined in `copy_internal`", is not wrong so much as
ambiguous: `lchownat` is at **3180**, and 3186 is the bare `dst_name` argument —
the second half of the same sentence. One citation was carrying two claims eight
lines apart. Noticing that pair is what started the sweep in
`49c8f277d`, which read all 192 upstream citations in `userspace/coreutils` and
`scripts/` against the real coreutils-9.4 source and corrected 66 of them.
