## `A-DEVFS-READ-FILE-AND-READ-AT-SERVE-DIFFERENT-NODES` (lane A, 2026-08-26) — ✅ FIXED 2026-08-26

**Fixed** in `f43685b4a`, by the first of the two options below — but not by
extending the second table to match the first. `read_file` no longer *has* a
table: it looks the node up in `DEV_NODES` and hands the read to `read_at`.
That is the substance of the fix. Two hand-written dispatch tables over one
node set will drift again no matter how carefully they are reconciled today;
one table cannot drift from itself.

Three things worth carrying forward:

* **The guard is `parent().is_empty()`, not `CharDevice`.** Two refusals had
  to survive the delegation, and only the root-only test preserves both: block
  devices stay whole-file-refused (so a recursive walk cannot become a 500 GB
  read), and the syscall-layer nodes under `input/`, `dri/` and `snd/` are
  intercepted at `open`, so devfs must keep answering "not that way". A plain
  `CharDevice` test would have quietly served both.
* **The mount root had to be special-cased back in.** `find_node("")` is
  `None`, so the delegating version reported `NotFound` for a directory that
  plainly exists — a regression I wrote and caught before the boot test. It
  now returns `IsADirectory` via an explicit early return, matching every
  other path in the file.
* **One observable length changed.** A whole-file read of `/dev/random` or
  `/dev/urandom` yields 4096 bytes rather than 256. Any bound on an endless
  stream is arbitrary; no caller in the tree reads either node whole, and none
  could depend on a particular count of random bytes being the last ones.

The self-test now drives **both** paths across all eleven root nodes rather
than `read_at` alone, plus the two refusals as explicit checks. Asserting only
the surviving path is what let the split live undetected in the first place.

---

**Original report.**

**In short:** There are two ways to read a file in this kernel — ask for the
whole thing (`read_file`), or ask for a byte range (`read_at`). For `/dev`,
those two answer differently about *which devices exist*. Reading all of
`/dev/kmsg` is refused; reading the first 64 bytes of it works. Nothing is
corrupted and nothing crashes — a caller that gets the refusal is told
`NotSupported`, which is a coherent "use the other path" — but the split is
undocumented and a caller has no way to know which nodes are on which side.

`kernel/src/fs/devfs.rs`. `read_file` (`match rel`) serves `null`, `zero`,
`full`, `random`, `urandom`, `console`, `tty` and nothing else at the root;
`stdin`, `stdout`, `stderr`, `kmsg` and `uptime` fall to `_ => unserved(rel)`,
which returns `NotSupported` for a node that exists. `read_at` serves **all**
of them, including `kmsg` (drains the klog ring) and `uptime` (formats elapsed
time).

**Why it is not simply a bug.** `unserved`'s doc comment makes the case for the
split deliberately, for block devices: `NotFound` "for a device `stat` just
described would send the caller hunting for an absent node instead of using
`read_at`". A whole-file read of an endless stream is meaningless, so refusing
it is defensible. What is *not* defensible is that the line between the two
sets looks arbitrary: `/dev/zero` is just as endless as `/dev/kmsg`, and it is
served by both.

**The proper fix** is to decide the rule and apply it uniformly. Two coherent
choices:

* **Every root character device serves both**, with `read_file` returning a
  bounded chunk — which is what `zero`/`random` already do (4096 and 256 bytes
  respectively). Cheapest, and makes `cat /dev/kmsg` work through the VFS.
* **No root character device serves `read_file`**, all of them answering
  `NotSupported` so a caller must use `read_at`. More honest about the fact
  that these are streams, but it breaks any existing caller reading `/dev/zero`
  whole, and the devfs self-test does exactly that.

The first is recommended: it matches the majority of the current behaviour and
breaks nothing.

**How it was found.** Writing the regression rung for
`A-DEVFS-NULL-AND-ZERO-STAT-AS-REGULAR-FILES`. The rung's first draft asserted
the eleven retyped nodes were readable via `read_file` and would have failed on
four of them. It was rewritten to use `read_at`.

**Not a regression.** `read_file` dispatches on the node's path and never on
its `EntryType`, so retyping the nodes to `CharDevice` did not change which
side of the split any of them is on. True since the nodes were added.
