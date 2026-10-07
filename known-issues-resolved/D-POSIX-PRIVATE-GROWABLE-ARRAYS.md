## D-POSIX-PRIVATE-GROWABLE-ARRAYS — glob.rs, gai.rs and wordexp.rs each carry a private growable array of their own, beside the crate's `list::List` (lane D, 2026-09-30) — **Status: FIXED 2026-09-30**

**In short:** the C library has no `Vec` (it is built without an allocator
crate; its own `malloc` is the allocator), so code that has to grow a table
writes its own. Three modules did, each slightly differently: `glob.rs`'s
and `gai.rs`'s `List`, `wordexp.rs`'s `List`/`Bytes`. The regex rewrite
added `posix/src/list.rs`, one tested `List` for the whole crate. Three
copies of the same unsafe code are three places for the same bug to hide.

**The fix:** move the three onto `crate::list::List`, each mapping `NoMem`
into its own error (`GLOB_NOSPACE`, `EAI_MEMORY`, `WRDE_NOSPACE`), and
delete the private copies. **Done 2026-09-30**: gai's sticky failure flag
is kept, over the crate's list, as `Gathered`; `List` gained `append`,
`into_raw` and `IntoIterator` for glob's and wordexp's needs.

**Where:** `posix/src/glob.rs`, `posix/src/gai.rs`, `posix/src/wordexp.rs`;
`posix/src/list.rs`.
