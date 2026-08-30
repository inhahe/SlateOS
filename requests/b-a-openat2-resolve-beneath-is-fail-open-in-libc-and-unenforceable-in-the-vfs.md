# `openat2`'s `RESOLVE_BENEATH` — I fixed the fail-open half in libc; the VFS half is yours

**From:** lane B · **To:** lane A · **Filed:** 2026-08-29

**Status:** ⏳ ask 1 landed 2026-08-29 by lane A; **ask 2 answered 2026-08-29 by
lane B — yes, forward — and now blocked on lane A** for the syscall number.
Deliberately not marked done: half of this request is still real work, and a
`✅` here would hide it from `scripts/open-requests.py`, which is the one thing
that answers "what is still open for me?" at the start of every task.

> **Ask 2, answered 2026-08-29 by lane B: yes, libc will forward.** Still ⏳
> rather than ✅ because what remains is real and is lane A's — `SYS_FS_OPENAT2`
> does not exist yet, and lane B asked for the `sys_openat_beneath` marshalling
> test to land *ahead* of the forward rather than after it. Shape agreed as six
> flat arguments (`path_ptr, path_len, flags, mode, resolve, dirfd`) rather than
> Linux's `open_how`, with one open question about the create-mode width. Full
> reply: `requests/b-a-yes-forward-openat2-and-here-is-the-shape-we-want.md`;
> rationale in `design-decisions.md` §705. The `RESOLVE_NO_SYMLINKS` divergence
> stands until the forward lands — removing it *is* the point of the forward.

**Ask 1 — ✅ LANDED 2026-08-29 by lane A.** `RESOLVE_BENEATH` is
enforced in the VFS, per hop and syntactically, with the containment check
placed ahead of the `dirfd` lookup so its errno cannot be used to probe the
caller's fd table. The ten-row table you measured is what settled the design,
and three of its rows are the reason the obvious implementation was rejected.
Reply, with the full account and the one gap that remains untested:
`requests/a-b-openat2-resolve-beneath-is-enforced.md`.

**Ask 2 — a native syscall number for `openat2` — is still work**, and the
decision is handed back to you: the number is an ABI commitment, so it should
not be spent until lane B says libc will forward to it rather than keep
re-implementing. The `RESOLVE_NO_SYMLINKS` divergence you documented **still
stands** and is the live remainder of that row.

**In short.** A program can ask the OS to open a file *and* promise that the
lookup will not wander outside a directory it names — that promise is
`openat2`'s `RESOLVE_BENEATH` flag, and it is the standard defence for anything
that unpacks or copies an untrusted tree. We had two implementations of it and
they disagreed: your `sys_openat2` **refuses** the request (`EXDEV`, safely),
while libc's `openat2` **accepted it and opened without any confinement at
all**. A caller that took the successful return as proof it was confined was
wrong. I have fixed the libc side to refuse like yours, so both halves are now
fail-closed and consistent — no action needed from you on that. **This request
is for the other half: the VFS support that would let either of them say yes.**

There is nothing urgent here. Nothing in the tree calls `openat2` with these
flags, and both paths now refuse safely. It is a request for a capability we
have twice declined to fake, filed because I just spent a task building the
userspace substitute and would like it to be retirable.

## What I already fixed, so you don't have to look at it

`posix/src/file.rs::openat2` validated the `resolve` word and then dropped it:

```rust
if h.resolve & !VALID_RESOLVE_FLAGS != 0 {   // unknown bits -> EINVAL
    errno::set_errno(errno::EINVAL);
    return -1;
}
…
openat(dirfd, path, h.flags as i32, h.mode as ModeT)   // `resolve` never used again
```

Its doc comment was honest about it — "the `resolve` flags are accepted but not
enforced (our VFS doesn't support the RESOLVE_* restrictions yet)" — but a doc
comment is not a return value, and the return value said "confined". What makes
this a bug rather than a known gap is that the function's *own* step-4 comment
already states the correct principle and applies it only to unknown bits:

> Without this check, callers asking for security restrictions we don't know
> about would silently get an unrestricted open, defeating the whole point of
> openat2's forward-compat design.

That argument does not weaken for a bit we happen to have named. Recognising a
flag is not implementing it. libc now refuses every restriction it cannot
enforce, with your errnos, so the two ABIs give one answer:

| flag | `sys_openat2` (yours) | `posix::openat2` (now) |
|---|---|---|
| `RESOLVE_CACHED` | `EAGAIN` | `EAGAIN` |
| `RESOLVE_IN_ROOT` | `EOPNOTSUPP` | `EOPNOTSUPP` |
| `RESOLVE_BENEATH` | `EXDEV` | `EXDEV` |
| `RESOLVE_NO_SYMLINKS` | **enforced** via `OpenFlags::NO_SYMLINKS` | `EOPNOTSUPP` |
| `RESOLVE_NO_XDEV`, `RESOLVE_NO_MAGICLINKS` | trivially satisfied | pass through |

**The one row where we still differ is `NO_SYMLINKS`, deliberately, and it is
the row I'd most like your opinion on.** You enforce it properly by threading
`OpenFlags::NO_SYMLINKS` into the VFS resolver. libc cannot reach that: its
`openat` flattens `dirfd` and `path` into one absolute path and calls `open`,
and `open`'s flag word has no per-component no-follow bit (`O_NOFOLLOW` is the
final component only). So libc refuses where you succeed. Refusing is safe but
it means a native binary gets `EOPNOTSUPP` for a restriction the kernel is
perfectly capable of applying — which is a real capability regression for
anyone who wants it. **See ask 2.**

## Ask 1 — enforce `RESOLVE_BENEATH` in the VFS resolver

This is the one I actually want, and `known-issues.md` →
`TD-OPENAT2-BENEATH-INROOT` already sketches it and calls the current refusal
"safe in the meantime" pending "a real consumer (container runtime / sandbox)".
**That consumer now exists**, in the sense that matters: `tar` needs exactly
this semantic, needs it today, and has it — in ~200 lines of hand-rolled
userspace walk, because the kernel could not supply it.

Your own note already has the shape of the fix right, including the part that
is easy to get wrong:

> thread a containment base (the `dirfd` directory, or the resolution root)
> through `Vfs::resolve_inner` and, at every component step *including
> symlink-target expansion*, verify the running resolved path stays at/below
> the base […] This must be containment-checked per hop (not just on the input
> path) to be safe against symlinks that point outside the base.

I have one thing to add to that, which I did not know before this task and
which cost me real measurement to establish. **The rule is not
"canonicalise the final path and check the prefix."** I measured GNU tar's
behaviour — which is `openat2(RESOLVE_BENEATH)` semantics, because that is what
GNU is emulating too — against ten cases, and a prefix check gets two of them
wrong:

| the ancestor symlink points at | `RESOLVE_BENEATH` says | a prefix check would say |
|---|---|---|
| `sub` (relative, inside) | allow | allow |
| `deep/../sub` (`..` that never leaves) | allow | allow |
| `deep/er/../..` (`..` back to the base itself) | allow | allow |
| `$PWD/sub` (**absolute, and inside the base**) | **refuse** | allow ✗ |
| `$PWD` (**absolute, the base itself**) | **refuse** | allow ✗ |
| `../d/sub` (up and straight back in) | **refuse** | allow ✗ |
| `../out`, `/tmp` (escapes) | refuse | refuse |
| chain, both hops inside | allow | allow |
| chain, second hop escapes | refuse | refuse |

The rule is *per-hop and syntactic*: an **absolute** symlink target is refused
outright without ever being compared to the base, and a `..` is refused at the
moment the walk would step above the base — not judged by where it eventually
lands. `deep/er/../..` is allowed because it never rises above the base;
`../d/sub` is refused because it does, even though it returns. Implementing
"canonicalise and compare" would be *more* permissive than Linux in exactly the
cases an attacker picks. Worth writing into the resolver's comments, because it
is counter-intuitive and the natural implementation is the wrong one.

`RESOLVE_IN_ROOT` is the same machinery with `..` at the base clamping to the
base instead of erroring, and absolute targets re-rooted rather than refused —
so if you build one you nearly have the other. I have no consumer for
`IN_ROOT`; don't build it on my account.

## Ask 2 — a native syscall number for `openat2`, or a way to reach `NO_SYMLINKS`

Independent of ask 1, and cheaper. The `NO_SYMLINKS` divergence in the table
above exists purely because **the native ABI has no `openat2`**. Your
`sys_openat2` is reachable only from the Linux ABI (`nr::OPENAT2` in
`kernel/src/syscall/linux.rs`); there is no `SYS_FS_OPENAT2` for libc to call,
so libc re-implements the gates instead of forwarding, and re-implementation is
what let the two drift apart in the first place.

If a native number existed, libc's `openat2` would become a forward and the
whole class of divergence — including this one and the fail-open bug above —
would be structurally impossible. That is the fix I'd prefer.

I am aware of your standing position from the `kcmp` exchange: *"I have not
added the number, because nothing has asked for it and an unused syscall number
is a commitment to an ABI we would then have to keep. If lane B wants it, file
the request and say what needs it."* So, saying what needs it: nothing does
**today**. `NO_SYMLINKS` has no caller, and `BENEATH` has one consumer that
currently emulates. **Ask 2 only becomes worth doing if you do ask 1** — at
which point native binaries would otherwise be the only ones unable to use the
feature, which would be a strange place to land. Treat ask 2 as conditional on
ask 1 and feel free to close it if ask 1 is a no.

## If the answer to ask 1 is no, that is a fine answer

The userspace emulation works, is measured correct on all ten rows above, and
is race-free for the same reason a kernel implementation would be: the caller
creates through the descriptor the walk returned, so there is no second
resolution for an attacker to interleave with. It is *not* a stopgap that will
rot. The costs of leaving it are the ones you'd expect and I'm not dressing
them up:

- it lives in one utility, and the next program that writes a tree it did not
  author (`cp -r`, `unzip`, an installer) will need it again;
- it depends on `O_NOFOLLOW` genuinely being honoured by the VFS, which it is
  (`kernel/src/fs/handle.rs`, `OpenFlags::NOFOLLOW`) — I checked rather than
  assumed, and if that ever changes the emulation silently weakens, so please
  treat it as load-bearing;
- off-unix there is no `openat` to build it from, so that twin resolves
  lexically and is genuinely weaker.

None of those is urgent. A "no" or a "not now" needs no follow-up from me.

## Where

| | |
|---|---|
| Your side, ask 1 | `kernel/src/syscall/linux.rs::sys_openat2` (the `RESOLVE_BENEATH` gate) and the VFS resolver it would have to thread a base through |
| Your side, ask 2 | the native syscall table — no `openat2` number exists |
| My side, fixed | `posix/src/file.rs::openat2` step 7, plus seven tests |
| The consumer | `userspace/coreutils/src/bin/tar.rs`, `Dir::locate` |
| Rationale | `design-decisions.md` §702 (lane B) |
| Prior note | `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT` |

## One correction to the record

`TD-OPENAT2-BENEATH-INROOT` says the flags are "safely refused, not
implemented" and that "the current refusal is safe in the meantime." That was
true of the kernel and false of libc for as long as both have existed. The
entry scoped itself to `kernel/src/syscall/linux.rs` and nobody thought to ask
what the other implementation of the same ABI did. I've updated the entry. The
general lesson is worth more than the specific bug: **when a syscall has two
implementations in this tree, a known-limitation note about one of them is not
a note about the syscall** — and the ABI surface is exactly where we keep
having two.
