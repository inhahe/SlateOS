## `B-tar-WALKS-THROUGH-A-PRE-EXISTING-SYMLINK-AND-WRITES-OUTSIDE-THE-DESTINATION` (lane B, 2026-08-29) -- **fixed 2026-08-29**, security

**In short:** unpacking an archive is supposed to put files *under* the
directory you unpacked it in, and nowhere else. Ours does not guarantee that.
If a symbolic link (a name that stands for another location, like a shortcut)
already exists in the destination and points somewhere outside it, our `tar`
happily follows it and writes the archive's files at the far end -- outside the
directory, possibly anywhere on the disk. GNU tar refuses. Two archives are
enough to exploit it: the first plants the link, the second writes through it.

### Measured, both sides

Destination `d` contains a pre-existing `x -> ../out`; the archive holds one
member `x/pwned`:

```
gnu : rc=2  tar: x/pwned: Cannot open: Invalid cross-device link
            out holds: []
ours: rc=0  (silent)
            out holds: [pwned]
```

And the two-step attack, which needs no pre-existing anything -- archive 1 holds
the single member `x -> ../out`, archive 2 holds `x/pwned`:

```
gnu : rc=2  tar: x/pwned: Cannot open: Invalid cross-device link   out: []
ours: rc=0  (silent)                                              out: [pwned]
```

`scripts/tar-diff.sh` does not catch it: its `prep_symlink_out` fixture puts a
symlink *at* a member's own path, where both tars replace it. Nothing in the
harness puts one on a member's **ancestor**, which is the only position from
which a link is followed rather than replaced.

### What we already defend against, and why it is not this

`tar.rs` implements GNU's *delayed-link* rule: a symlink member whose target is
absolute or contains `..` is not created when it is read, it is stood up as an
empty placeholder file and turned into a symlink after the last member. So an
archive holding `x -> /etc` followed by `x/passwd` writes nothing to `/etc` --
at the moment `x/passwd` is opened, `x` is a regular file.

That defence covers exactly one run. It says nothing about a link that was
already on disk when tar started, and -- since the placeholder *is* replaced by
the real symlink at the end of the run -- it says nothing about the second run
either. The first archive of the two-step attack is not even hostile-looking:
it is one ordinary symlink member, created exactly as GNU creates it.

### GNU's rule, measured exactly

It is `openat2(RESOLVE_BENEATH)` semantics, component by component -- **not**
"canonicalise and check the prefix". With a pre-existing ancestor `d/x` and
member `x/pwned`, extracting into `d`:

| `d/x` points at | verdict |
|---|---|
| `sub` (relative, inside) | allowed |
| `deep/../sub` (`..` that never leaves) | allowed |
| `deep/er/../..` (`..` back to the root itself) | allowed |
| `/abs/path/d/sub` (absolute, **inside**) | **refused** |
| `/abs/path/d` (absolute, the destination root itself) | **refused** |
| `../d/sub` (up and straight back in) | **refused** |
| `../out`, `/tmp` (escapes) | **refused** |
| `x -> y`, `y -> sub` (chain, both hops inside) | allowed |
| `x -> y`, `y -> ../out` **or** `y -> /abs/d/sub` | **refused** |

So: an **absolute** symlink target is refused whatever it points at, even at the
destination root; a relative one is refused the moment the walk would step
above the root, even if a later component comes back in; chains are followed
and every hop is judged by the same rule. That is precisely `RESOLVE_BENEATH`,
which is why the errno is the otherwise-inexplicable `EXDEV`.

`--overwrite` does not change it. Every member type is refused, each with its
own wording and all with the same errno:

```
tar: x/pwned: Cannot open: Invalid cross-device link
tar: x/sl: Cannot create symlink to 'elsewhere': Invalid cross-device link
tar: x/p: Cannot mkfifo: Invalid cross-device link
tar: x/d: Cannot mkdir: Invalid cross-device link
```

all followed by `Exiting with failure status due to previous errors`, rc 2.

It is tar's own doing and not the environment: `tar-sanity.sh` writes through
the very same link with a shell redirect and with `cp`, both fine. Measured
against tar 1.35 on kernel 6.6.87.2-microsoft-standard-WSL2; probes
`tar-rules12.sh` through `tar-rules16.sh` and `tar-sanity.sh` in the scratch
reference directory.

Note that `tar-rules14.sh`'s first table is **not** evidence for the absolute
rule and was misread once: its "absolute, in tree" and "up and back in" targets
were written as `$PWD/x/sub` and `../x/sub`, which pass back *through* `x`, the
link under test -- resolution loops, refused for a reason that has nothing to do
with the rule. `tar-rules16.sh` re-runs the table with targets that never
re-enter the link, and that is what distinguishes `RESOLVE_BENEATH` from
canonicalisation: canonicalisation would allow absolute-but-inside and
`../d/sub`, and GNU refuses both.

### The proper fix

Resolve every member's **parent directory** beneath the destination root, and
create the leaf with an `*at()` call relative to the resulting descriptor --
which is GNU's architecture, and the reason its `mkdir`, `symlink` and `mkfifo`
failures all report `EXDEV` too: none of `mkdirat`/`symlinkat`/`linkat` takes a
resolve flag, so the restriction can only live in the resolution of the parent.

`openat2(RESOLVE_BENEATH)` cannot be the mechanism here:

* on the **SlateOS** target, `posix/src/file.rs` has `openat2` and the
  `RESOLVE_*` constants, but -- in its own words -- "delegates the actual open
  to regular `openat` once validation passes; the `resolve` flags are accepted
  but not enforced (our VFS doesn't support the RESOLVE_* restrictions yet)".
  Enforcing them is a VFS change, and the VFS is lane A's.
* on the **host** build, glibc exports no `openat2` wrapper at all, so it would
  mean `syscall(437, ...)` by number.

Neither is needed. The restriction is emulable in userspace, race-free, out of
primitives that both targets already have (`openat`, `readlinkat`, `mkdirat`,
`symlinkat`, `linkat`, `mkfifoat`, `mknodat`, `unlinkat`, `fchmodat`,
`fchownat`, `utimensat` -- all present in `posix/src/file.rs` and in glibc):
walk the parent's components from a held root descriptor, opening each with
`O_DIRECTORY|O_NOFOLLOW` so a symlink component fails rather than being
followed; on that failure `readlinkat` it, refuse an absolute target outright,
and splice a relative one's components into the walk under a loop counter;
keep the descriptors in a stack so `..` pops it and popping the root is the
refusal. Holding a descriptor per level is what makes it race-free -- a
check-then-create by path would be correct on a quiet disk and defeatable by
an attacker who swaps a component in between.

### Fixed

Done as described. `tar.rs` gained `Dir` (the destination, held open) and
`Located` (a parent descriptor plus the one component naming the member in it);
`Dir::locate` is the walk above and every creation, stamp and removal in the
extraction path is now an `*at()` call through the descriptor `locate` returned.
`create_at` replaces `create_recovering`: it resolves the parent first and only
invents ancestors on the `ENOENT` that says one is genuinely absent, so the
`mkdir -p` that used to run ahead of the open -- and would have made a directory
where a withheld symlink's placeholder stood -- is gone.

All ten rows of the table above now match GNU exactly, refusals and permissions
alike, with the same errno and the same per-type wording; so does the hard
link whose *target* escapes, which is refused because `link_target` is resolved
through the same `locate`. `scripts/tar-diff.sh` carries twelve new fixtures
(`an ancestor symlink ...`, `a chain whose second hop escapes`, `a symlink
planted by an earlier archive`, `a hard link target reached through an escape`)
and its existing "nothing was written outside the destination" check now runs
unconditionally rather than only when python3 was available to forge headers.
Verified to discriminate: rebuilt against the pre-fix `tar.rs`, eight of the
twelve fail and the leak check reports `escape/pwned escape/sl escape/p
escape/dir`. 90 pass, 0 differ, 3 differ on purpose.

Two things found along the way, both fixed here:

* `identity()` was `openat(O_RDONLY|O_NOFOLLOW)` + `File::metadata`, chosen to
  avoid declaring a `struct stat` by hand. That breaks the delayed-symlink
  placeholder outright: the placeholder is created **mode 0**, and an
  unprivileged process cannot open a mode-0 file even when it owns it, so every
  archive containing an absolute or climbing symlink failed with
  `tar: x: Cannot open: Permission denied`. It is `fstatat(AT_SYMLINK_NOFOLLOW)`
  now. `posix/src/stat.rs` documents its `Stat` as matching Linux x86-64's, so
  one declaration serves both targets; a `const` assertion on its size fails the
  build if that ever drifts.
* `locate` returned `EINVAL` for a name with no components, which is what
  `tar -cf x.tar .` stores (`./`). The destination is a location, not an error:
  the leaf is `.` relative to the root's own descriptor, and `mkdirat`/`openat`/
  `linkat` then produce GNU's `EEXIST`/`EEXIST`/`EPERM` of their own accord.

Off unix there is a twin that resolves lexically and creates by path. It is a
genuinely weaker guarantee -- a member cannot be *named* outside the
destination, but a link planted between the check and the creation would not be
caught -- and it is documented as such at the type. That platform cannot create
a symlink, a fifo or a device node in the first place.
