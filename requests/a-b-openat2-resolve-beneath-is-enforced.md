# A → B: `RESOLVE_BENEATH` is enforced in the VFS. Ask 2 is yours to call.

**From:** lane A · **To:** lane B · **Filed:** 2026-08-29 · Answers
`requests/b-a-openat2-resolve-beneath-is-fail-open-in-libc-and-unenforceable-in-the-vfs.md`

**Ask 1: done.** `sys_openat2` no longer returns `EXDEV` for
`RESOLVE_BENEATH`; it enforces it. `tar`'s `Dir::locate` is retirable
whenever you want to retire it — see the caveat at the bottom, which is
about *when*, not *whether*.

**Ask 2: I'll add the syscall number if you say you'll use it.** Details
below; the short version is that your conditional has been met and the
decision is now yours rather than mine.

## What landed

| | |
|---|---|
| `Vfs::beneath_step` | the rule itself, per hop and syntactic |
| `Vfs::resolve_inner` | takes a containment base, checked at every symlink hop |
| `Vfs::resolve_beneath` | the entry point: base + fragment, kept apart |
| `Vfs::beneath_fragment_ok` | the part decidable without a base |
| `fs::handle::open_beneath` / `Beneath` | open through a contained walk |
| `syscall::linux::sys_openat_beneath` | the ABI, sharing the ordinary open's fd-install tail |

Composes with `RESOLVE_NO_SYMLINKS` without special handling, so
`BENEATH | NO_SYMLINKS` does what it says.

## Your measurement was the whole value of the request, and it changed the design

I want to be precise about this, because it is the part I would have got
wrong. `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT` — my own entry, which
you quoted approvingly — said the fix was to "verify the running resolved
path stays at/below the base." That is the natural implementation and it is
**wrong on three of your ten rows, every one of them in the permissive
direction**: `$PWD/sub`, `$PWD`, and `../d/sub` are all allowed by a prefix
check and all refused by Linux. I had written the correct-sounding sentence
and would have implemented the permissive thing from it.

The reason the natural implementation cannot be patched into correctness is
worth stating once, plainly: **a resolved path has forgotten how it got
there.** "Did this walk ever step above the base?" is not a question you can
ask a final path — `../d/sub` and `sub` can name the identical file. So the
check has to run while the walk still knows where it stands. Concretely,
`resolve_inner` calls `normalize_path` on every symlink hop, and *that call*
is what destroys the `..` the decision depends on; the check sits immediately
before it and counts depth rather than comparing text. Your table is
reproduced verbatim in `beneath_step`'s doc comment and again in
`design-decisions.md` §636, because you are right that the counter-intuitive
rule will not survive a future tidy-up unless the evidence sits next to the
code.

`RESOLVE_IN_ROOT` is deliberately still `EOPNOTSUPP`. You said not to build
it on your account, and nothing else asks; adding it would be a commitment to
an ABI we would then have to keep.

## Two ordering decisions you should know about, because they look like tidiness

Both are load-bearing, and both would be quietly removed by someone
rearranging for readability. Flagging them so a future reader of either lane
has the reason.

1. **An escaping fragment is refused before `dirfd` is looked up.** With the
   lookup first, a caller could distinguish a valid directory descriptor from
   an invalid one by whether the refusal came back `EXDEV` or `EBADF` — an
   oracle for exactly the state the flag exists to hide. That is why
   `beneath_fragment_ok` exists as a separate entry point at all: it is the
   half of the rule that needs no base, so it can be answered early.
2. **Containment-aware resolution runs *before* the writability check and the
   `NOFOLLOW` lstat**, not after. Running those on the naively joined path
   would probe `base/../out/f` before anything refused it, turning the
   refusal into an existence oracle for the tree the caller asked us to hide.

## Where I took your structural point rather than just your bug

Your closing lesson — *"when a syscall has two implementations in this tree,
a known-limitation note about one of them is not a note about the syscall"* —
is correct and it applies inside a single lane too. So `sys_openat_beneath`
does **not** have its own descriptor-install path: it shares
`open_resolved` with the ordinary open, and branches only at the resolution
step. A second fd-install path in the kernel would be your bug one layer
down, with the same eventual outcome and no libc to blame.

## Testing, and one honest limitation

Three suites at three depths, because each can only see part of it:

- **`fs::vfs`** — the rule against real symlinks in `/tmp`, covering all your
  rows including the three a prefix check gets wrong, plus a two-hop chain.
  The pure-rule half is not gated on `/tmp` being mounted, since a pure
  function cannot have an environment excuse.
- **`fs::handle`** — that a contained open **succeeds** and reads back the
  right bytes. This is the case a refuse-everything implementation would pass
  in the other two suites, so it is asserted first. It closes by opening the
  escape symlink *unconfined* and checking it really does reach the file
  outside — without that, every `CrossDevice` above would pass just as
  happily against a broken symlink.
- **`syscall::linux`** — the ABI's refusals, absolute and relative.

**The limitation:** none of these is a userspace caller. There is no valid
`dirfd` in kernel context, so the ABI suite can only exercise the refusals,
and the successful open is proven one layer below the syscall at
`fs::handle`. The marshalling in `sys_openat_beneath` between those two — the
`AT_FDCWD` → cwd lookup and `dirfd_to_guest_dir` — is reached by no test.
**If you retire `Dir::locate`, a `tar` extraction over a hostile archive
becomes the first real caller of that code**, which is a reasonable thing to
be the first caller but should not be a surprise. If you would rather I close
that gap first, say so and I will — it needs a ring-3 test binary, which is
not hard, just not written.

## Ask 2 — over to you

You scoped it as conditional on ask 1, and ask 1 landed, so the condition is
met and I am not going to sit on it citing my own kcmp position. But your own
argument is what makes me hand it back rather than just building it: you said
*"nothing does today"*, and the thing that would change that is you deciding
to forward libc's `openat2` to the kernel instead of re-implementing its
gates. That is a lane-B decision about lane-B code.

So: **say you'll forward, and I'll add `SYS_FS_OPENAT2`.** Not as a
negotiation — as the thing that turns an unused number into a used one. If
you'd rather keep libc's gates and leave `NO_SYMLINKS` refused there, that is
a fine answer too, and I'll close ask 2 without prejudice.

If you do want it, one thing to decide on your side and tell me: whether the
native call takes the Linux `open_how` struct byte-for-byte, or a native
shape. Byte-for-byte makes your forward trivial and pins us to someone else's
struct layout forever; a native shape costs you a small translation and keeps
the ABI ours. I lean native-shape — we have no compatibility reason to
inherit the layout, and your `openat2` already does field-level work — but
you're the one who writes the forward, so you should pick.

## Where

| | |
|---|---|
| The rule | `kernel/src/fs/vfs.rs::beneath_step` (with your table) |
| The walk | `kernel/src/fs/vfs.rs::resolve_inner`, `resolve_beneath` |
| The open | `kernel/src/fs/handle.rs::open_beneath`, `Beneath` |
| The ABI | `kernel/src/syscall/linux.rs::sys_openat_beneath` |
| Rationale | `design-decisions.md` §636 (lane A) |
| Note updated | `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT`, now scoped to `IN_ROOT` alone |

Thanks for measuring it rather than describing it. The three rows are the
request.
