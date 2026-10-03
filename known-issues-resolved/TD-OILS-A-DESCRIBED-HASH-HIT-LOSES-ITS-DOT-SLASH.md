### TD-OILS-A-DESCRIBED-HASH-HIT-LOSES-ITS-DOT-SLASH. bash writes `./bin/tool` where osh writes `bin/tool` — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::hashed_description`, the same
description path as the entry above, and fixed with it: the divergence is only
reachable once a hash hit is reported at all.

**Reproduce:**

```sh
mkdir -p bin; printf '#!/bin/sh\necho x\n' > bin/tool; chmod +x bin/tool
PATH=bin; hash -r; tool > /dev/null
hash                # bash lists  bin/tool   — the stored path, raw
command -v tool     # bash prints ./bin/tool — the described path
```

**Why (measured, replacing the original guess).** The original entry blamed
bash's `sh_makepath(MP_DOCWD)` at *store* time and claimed `hash`'s own listing
also shows `./bin/tool`. Neither is so. The table stores the search's own
spelling — `bin/tool` — and `hash`'s listing prints it raw. The `./` is added at
**description** time, and only under two conditions: the stored path is
*relative*, **and** `./` + stored is *currently executable*. The executability
check is made afresh on every description and is relative to the shell's cwd, so
a `cd` or a deleted file changes the answer without changing the table. An
absolute entry never gets a `./`.

**The fix.** `hashed_description` implements exactly that: return the stored
path unchanged if `path_is_rooted`, else probe `./` + stored via
`path_is_executable(self.probe_path(..))` and return the dotted form only if it
still runs. The unit test pins all four cases (relative-and-runnable,
after-a-`cd`, after-the-file-is-removed, absolute).
