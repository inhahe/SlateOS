## A-SYSINFO-REPORTS-AN-OS-IDENTITY-NOBODY-MEASURED (lane A, 2026-09-15) — **Status: the website is FIXED; four fields remain open**

**What it is.** `kernel/src/fs/sysinfo.rs::init_defaults` fills `OsInfo` with six values, none of which the kernel measures, and `kshell`'s `sysinfo` and `sysinfo os` commands print them to the operator as facts.

| field | value | verdict |
|---|---|---|
| `website` | `https://mintos.dev` | **fixed** -- names the project's former name and no such site is published |
| `build_date` | `2026-05-06` | **open, and false** -- the kernel running today was not built in May |
| `build_number` | `1` | open -- inert, has never been incremented |
| `version` / `kernel_version` | `1.0.0` | open -- arguably a policy placeholder rather than a measurement |
| `codename` | `Mint` | open -- same |
| `name` | `MintOS` | tracked separately; see the `/sys/kernel/ostype` entry above |

**Why the website was the one to fix first.** Lane C's test, from triaging the same shape across ten apps today: an inert setting gets a notice, a false statement gets corrected -- and separately, a value that claims an *external* resource exists is worse than one that is merely unset. A URL asserts that somebody is hosting something. The reader has no instrument that distinguishes an invented URL from a real one, which is exactly why it survived: a plausible string draws no reaction.

**The fix was already half-built in the file.** Both other `OsInfo` constructors leave `website` empty, and the `sysinfo` printer already omits the line when it is. So emit-or-omit was implemented and `init_defaults` was the single site violating it. The `sysinfo os` printer did *not* guard, so fixing the first half alone would have printed a label with nothing after it -- both were changed together rather than leaving the second to surface later as a regression.

**What the remaining fix looks like.** `build_date` and `build_number` want a build-time stamp (a `build.rs` emitting the date and a counter, read through `env!`), which makes them measurements instead of placeholders. Until that exists they should be empty and omitted, not invented: an absent build date is honest, and `2026-05-06` is not. `version`/`codename` are genuinely a naming decision and are left alone pending one.

**How it was found.** Not by a gate. A scan of the 495 `gen_*` functions in `procfs.rs`/`sysfs.rs` for any that emit without consulting a source -- prompted by lane C reporting the identical shape in `apps/`. The scan itself returned one hit (`gen_filesystems`), which turned out to be *correct*: all seven filesystems it lists have a module and live registrations. The real finding came from reading the file next to it.
