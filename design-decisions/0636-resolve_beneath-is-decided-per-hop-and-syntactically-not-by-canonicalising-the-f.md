## 636. `RESOLVE_BENEATH` is decided per hop and syntactically, not by canonicalising the final path

**Date:** 2026-08-29
**Decided by:** Claude (autonomous, lane A) — implementing ask 1 of
`requests/b-a-openat2-resolve-beneath-is-fail-open-in-libc-and-unenforceable-in-the-vfs.md`,
with lane B's measured evidence. That file is stamped rather than deleted (rule
2 / §315), so the evidence is still where the argument is; the reply — which
reproduces it — is `requests/a-b-openat2-resolve-beneath-is-enforced.md`.

**Lane:** A

**In short:** a program can ask us to open a file *and* promise the lookup will
not wander outside a directory it names — that promise is `openat2`'s
`RESOLVE_BENEATH` flag, and it is what a program unpacking an untrusted archive
uses so a booby-trapped entry cannot write outside the folder you chose. We
refused the request before; now we honour it. The decision recorded here is
*how* we check, because the obvious way to check is wrong, and wrong in the
direction that lets attacks through.

**The obvious implementation.** Follow the path wherever it goes, work out the
final real location ("canonicalise" it — resolve every shortcut and every `..`
until one plain path is left), and then check that the answer sits inside the
base directory. It is one comparison at the end, it is easy to reason about,
and every part of it is already written.

**Why it is wrong.** Lane B measured GNU tar — which emulates these exact
semantics — across ten cases. On three of them a final-path comparison
*disagrees with Linux, and always by allowing something Linux refuses*:

| the shortcut points at | `RESOLVE_BENEATH` | final-path comparison |
|---|---|---|
| `sub` (a name inside) | allow | allow |
| `deep/../sub` (goes down, back up, down — never leaves) | allow | allow |
| `deep/er/../..` (back up to the base itself) | allow | allow |
| `/the/base/sub` (spelled from the root, and **inside** the base) | **refuse** | allow ✗ |
| `/the/base` (spelled from the root, **is** the base) | **refuse** | allow ✗ |
| `../d/sub` (steps out of the base and straight back in) | **refuse** | allow ✗ |
| `../out`, `/tmp` (leaves and stays out) | refuse | refuse |

The rule those rows describe is not about *where you end up*. It is about *how
you got there*: a path spelled from the root is refused for being spelled from
the root, even when it names a file inside the base; and a `..` is refused at
the instant the walk would step above the base, not forgiven by a later
component that steps back down. `deep/er/../..` is allowed because it never
rises above the base; `../d/sub` is refused because it does, though it returns.

**Why the obvious implementation *cannot* be patched into correctness.** The
question "did this walk ever step above the base?" is not answerable from the
final path. A resolved path has forgotten how it got there — `../d/sub` and
`sub` can name the identical file. So the check has to happen while the walk
still knows where it stands. Concretely: `resolve_inner` calls `normalize_path`
on every symlink hop, and that call is precisely what collapses the `..` the
decision depends on. The check is therefore placed *before* it, at the hop, and
counts depth relative to the base rather than comparing text.

**What we gave up.** More code in the resolver's hot recursive path, a
containment base threaded through a function that had four parameters and now
has five, and a rule that reads as arbitrary until you see the table. Against
that: the alternative is more permissive than Linux in exactly the three shapes
an attacker picks, while looking correct and passing any test written by
someone who reasoned about it the obvious way. That is the worst failure mode
available — a security check that is confidently wrong. The table is reproduced
in `Vfs::beneath_step`'s doc comment for the same reason it is reproduced here.

**A second, smaller call, recorded because it is easy to undo by accident.**
`sys_openat_beneath` refuses an escaping *fragment* — an absolute path, or one
that opens with `..` — **before** it looks up `dirfd`. That ordering is not
tidiness: with the lookup first, a caller could tell a valid directory
descriptor from an invalid one by whether the refusal came back `EXDEV` or
`EBADF`, which is an oracle for exactly the state the flag exists to keep out
of reach. The same reasoning puts the containment-aware resolution *ahead of*
the writability check and the `NOFOLLOW` lstat in `open_impl`: running those on
the naively joined path would probe `base/../out/f` before anything refused it,
turning the refusal into an existence oracle for the tree the caller asked us
to hide. The check that comes first is part of the guarantee, not an
optimisation, and a future reader tidying the order would silently remove it.

**`RESOLVE_IN_ROOT` deliberately not built.** It is the same machinery with
`..` at the base clamping instead of erroring and absolute targets re-rooted
instead of refused — so having one, we nearly have the other. Lane B explicitly
has no consumer and said not to build it on their account. An unused syscall
capability is a commitment to an ABI we would then have to keep, and the
standing lane-A position (from the `kcmp` exchange) is that we add one when
something asks. Nothing does.
