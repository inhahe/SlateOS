### [A] Six fs metadata tables key on the path string, so metadata does not follow a hard-linked file -- and the ACL one is pre-positioned to become a permission bypass -- 2026-09-21
**Status:** OPEN (6 of 8 converted on `main`: flock, sealing, record locks, immutable flags, ACLs, `queryable` (478ee8c4c). `fcomment` fixed on `lane-a-wip` 2026-10-02, awaiting a boot on main: a comment is the file's own `user.xdg.comment` attribute, so it follows the inode with no table at all (design-decisions §1533). Remaining: `tags` (A-Q22). `integrity` reclassified as correctly path-keyed; `history` undecided)

**In short:** the kernel stores several kinds of per-file information -- ACLs,
comments, tags, version history, integrity hashes -- in side tables looked up
by the file's *name* rather than by the file itself. A file can have two names
(a hard link). Under the second name, none of that information is found. For
comments and tags that is a wrong answer; for ACLs it would be a way to walk
past a permission rule, if ACLs could be set from a program. They cannot, yet.

**How this was found, because the route matters.** I converted four tables
(flock, sealing, record locks, immutable flags) to key on
`FileId { fs_id, ino }` and wrote a boot gate for them. Only afterwards did I
look at how many such tables exist. Four was the number I happened to be
holding, not a measured population -- the same defect as dd-956, which I wrote
two days ago specifically about measuring a population before building a gate
for it. The tell was `immutable::rename_path`: a fixup that exists to drag a
path key along behind a rename. A grep for it found **three** copies, in
`immutable`, `fcomment` and `queryable`, each called only from its own
self-tests. One fixup is a quirk; three identical ones are a class.

**The measured population.** Every `BTreeMap<PathBuf, _>` under `kernel/src/fs`,
classified by whether the data describes the *file* or the *name*:

| module | keyed data | verdict |
|---|---|---|
| `acl` | POSIX ACLs | **should follow the inode** -- POSIX stores ACLs in the inode's xattrs, so path keying is a semantic deviation, not just a miss |
| `fcomment` | a comment on the file | should follow the inode |
| `history` | version history of the contents | **undecided** -- a real tradeoff, not a miss; see below |
| `integrity` | a content hash baseline | **correctly path-keyed -- I had this wrong, see below** |
| `queryable` | indexed attributes | should follow the inode |
| `tags` | user tags | should follow the inode |
| `dirsync`, `overlay`, `rundialog`, `undelete`, `usage` | comparisons between trees, overlay whiteouts, typed strings, records of deleted *names*, usage per directory | correctly name-keyed; an inode key would be wrong |
| `memfs`, `path` | a filesystem's own directory structure; path utilities | name-keyed by definition |
| `cap::file_tags` | capability-group tags | correctly **path**-keyed: `effective_tags` walks every ancestor, so a file's tags depend on where it lives. Inode keying would break inheritance |

So the population is 8 tables that hold per-file data and should follow the
inode, of which 5 are now converted (`acl` joined them today) and 3 are not,
plus 8 that are right as they are and 1 undecided.

**A correction, made before writing any code for it.** I first listed
`integrity` as needing conversion. It does not, and converting it would have
destroyed what it does. `verify_file` looks a baseline up by path, reads the
content *currently at that path*, and compares. The threat it detects is a file
being **replaced** -- and a replacement is a different inode. Key it by inode
and `baseline.get(id)` misses, so a swapped `/etc/passwd` reports "no baseline"
instead of `Modified`; worse, `VerifyStatus::Missing` becomes unreachable, since
a file that no longer exists has no inode to look up. Path is not a weaker key
here, it is the correct one. Tripwire and AIDE monitor paths for the same
reason.

The generalisation I had been using -- "per-file metadata should follow the
file" -- was too coarse, and it took reading `verify_file` to see it. The
sharper question is: **should this data survive the file at that path being
replaced?** Yes means path (integrity monitoring). No means inode (an ACL, a
comment, a tag -- none of which should transfer to a stranger's file that
happens to land at the same name).

`history` is left undecided on purpose. Version history could reasonably be
either: keyed by inode a rename keeps its history, which is what Dropbox and
macOS versions do; keyed by path you get the history of a location, which is
what a user watching one config file may expect. That is a genuine tradeoff
with a user-visible answer, so it is not mine to settle silently -- it wants an
`open-questions.md` entry before any code moves.

**A claim I nearly published, and the check that stopped it.** Having
established that `acl::check_access` is called from `vfs::path_access_verdict`,
which is called from `check_path_access`, which `handle.rs` calls on every open
for Read/Write/Metadata, I was about to file this as a **live permission
bypass**: set a restrictive ACL, hard-link the file elsewhere, open the link,
and `acls.get(path)` returns `None`, which means *allow* ("No ACL, defer to
traditional permissions"). Every link in that chain is real and I verified each
one.

It is still not a bypass, because of the link I had not checked: **there is no
ACL syscall.** `set_acl` has exactly two callers outside its module -- `kshell`
and one self-test -- and `grep` for `SYS_*ACL` or `setxattr` in
`syscall/number.rs` returns nothing. `path_access_verdict` guards the ACL call
with `if super::acl::count() != 0`, and in any boot where no human typed a
kshell command, that count is 0 and `check_acl` is never reached. No program
can create the precondition.

This is the third time in this codebase I have mistaken *on the live code path*
for *reachable by an attacker* -- `sealing` and `secpolicy` were the first two,
where I published "can only ever be 0" about counters that kshell moves. The
proxy is seductive because the call graph is genuine; what is missing is an
actor who can enter it (dd-953).

**Why it is still worth fixing, and fixing first.** The defect is
*pre-positioned*. The day someone adds `SYS_ACL_SET` -- a normal, unremarkable
roadmap item -- the bypass becomes live, and nothing in the tree would flag it,
because the new syscall would look correct in isolation and the table it writes
to has always been keyed this way. A latent hole that arms itself when an
unrelated feature lands is worse than a loud one.

**The fix** is the pattern already applied to the other four: derive
`Option<FileId>` once, above the lock (holding a module global across a VFS call
inverts filesystem-lock -> module-state and can wedge two CPUs -- 9 such sites
were introduced and caught by `check-vfs-under-lock.py` earlier today), match on
`(path, id)`, and fall back to the path when identity is unresolvable so a
not-yet-created file still works. `immutable.rs`'s `flag_key` is the reference:
one key-construction point, `FlagKey::Id | FlagKey::Path`. Each converted table
also needs a rung that hard-links a real file, since the pre-existing rungs used
synthetic paths that resolve to nothing and so passed identically before and
after conversion.

**A search-shaped mistake worth naming, found the same day.** Scoping the
BLKDISCARD work I ran
`grep -r 'fn discard' kernel/src/drivers kernel/src/block` and got nothing,
and reported "no discard/TRIM/unmap support anywhere". **Neither directory
exists.** `grep` over a non-existent path prints nothing and returns quietly,
and I read that silence as a measurement. The truth is the opposite of what I
published: `kernel/src/blkdev.rs` has had `supports_discard()` and `discard()`
on the `BlockDevice` trait all along, and **1 of the 4 implementors
(`RamBlockDevice`) overrides both**, so discard is genuinely available on one
backend and the design that follows is not "refuse everything" but "ask the
device".

The tell is that an empty result and an empty *search space* are printed
identically. Same family as reading exit 0 as a warning count, and as
`tail -25` of an 18,060-line report: in each case the evidence could not have
contained the finding, and nothing in the output said so. The cheap guard is to
make the search prove its own scope -- `ls -d` the paths first, or grep for a
term that MUST hit and check that it does.
**The three remaining are NOT the same mechanical change, measured 2026-09-21.**
This entry said "the fix is the pattern already applied to the other four".
That is true of one of them and misleading about the other two, which is worth
correcting because it is what the next session would act on:

| module | table(s) | difficulty |
|---|---|---|
| `queryable` | `path_index: BTreeMap<PathBuf, usize>` into `files: Vec<FileAttrs>` | **mechanical.** The index is a lookup; the records live in the Vec. Re-key the index and the rest follows |
| `fcomment` | `comments: BTreeMap<PathBuf, String>` | **not mechanical -- measured 2026-09-22.** 18 table accesses, but **28 compiler-reported sites** once the value becomes a tuple, because the comment IS the value: every `get`/`get_mut`/iteration changes shape. `search`/`list`/`remove_under` filter by path *prefix*, so the path must stay as data, and `remove_under` must collect KEYS rather than paths |
| `tags` | `by_path: BTreeMap<PathBuf, BTreeSet<String>>` **and** `by_tag: BTreeMap<String, BTreeSet<PathBuf>>` | **a design question, not a conversion.** Two indices over the same relation |

**`queryable` is done (2026-09-22); `fcomment` was attempted and reverted.** The
conversion compiled down to 28 sites of restructuring rather than a swap, and it
was attempted at a point where no boot could verify it -- WSL is down on this
host, which fails gate 50's main run. Reverted rather than landed blind: the
table is reachable only from `kshell`, so the value of landing it unverified is
low and the cost of debugging 28 unverified sites later is not. The measurement
is the artifact; the next session starts with a correct estimate instead of my
wrong one.

**Why `tags` is different.** Re-key `by_path` by identity and `by_tag` still
holds paths. Then one file with two names has *one* entry in `by_path` and
*two* in `by_tag`, so "which files carry tag X" and "which tags does this file
carry" stop being inverses of each other. Three ways out, none free:

| option | cost |
|---|---|
| key `by_tag` by `FileId` too, resolve to a path only for display | consistent, but "list files tagged X" must resolve N identities back to names, and a file deleted since tagging has no name to resolve to |
| keep `by_tag` by path and accept the asymmetry | cheap, and it reintroduces exactly the bug being fixed on the reverse lookup |
| store the path as data beside the id in both indices | the `immutable.rs` shape applied twice; duplicated paths must not drift apart |

I have not picked one. It needs an entry of its own rather than being folded
into a conversion that looks mechanical from the outside -- and the reason I
know is that I was about to stage it as a 10-site edit, `tags` having the fewest
call sites of the three. Fewest sites, most design.
**Correction to the line below: the three `rename_path` fixups become
CONDITIONAL, not dead.** An inode survives a rename, so identity keying makes
them unnecessary *for files that have a stable inode*. The path fallback exists
precisely for files that do not, and on those a rename still moves the key. So
each one should keep its body and gain a doc line saying it is now reached only
on the fallback path -- deleting them would silently drop metadata on any
filesystem without stable inodes. (`queryable`'s is at line 820; converting it
needs a key derived above the lock in 6 of its 8 index-touching functions --
`set_attr`, `get_attr`, `remove_attr`, `list_attrs`, `clear_attrs`,
`rename_path` -- while `stats` and `clear_all` only call `.len()`/`.clear()` and
need none.)

**Then delete the three `rename_path` fixups**, which identity keying makes
unnecessary: an inode survives a rename, so there is nothing left to fix up.
