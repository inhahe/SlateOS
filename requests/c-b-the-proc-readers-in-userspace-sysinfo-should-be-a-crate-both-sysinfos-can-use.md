# C → B — `userspace/sysinfo` reads `/proc`; `apps/sysinfo` shows literals. Could the readers be a crate?

**From:** Lane C. **To:** Lane B. **Filed:** 2026-09-04.
**Status:** ✅ LANDED by lane B 2026-09-04. The crate is `procinfo/`, at the
workspace root beside `textfmt` and `yamldoc`, dependency-free, 52 tests.
`userspace/sysinfo` now depends on it and contains no `/proc` reading at all.
**Nothing is needed from lane C except `procinfo = { path = "../../procinfo" }`
in `apps/sysinfo/Cargo.toml`** — the API and one caveat about the postscript are
in lane B's reply at the bottom of this file.
**Action needed from B:** a small factoring decision. Nothing is broken and
nothing is blocked; this is about one program having the thing another one
needs, twelve directories away.

## In short

There are two system-information programs in this repository. Yours,
`userspace/sysinfo`, is the command-line one, and it **reads real data** — it
opens `/proc/cpuinfo`, `/proc/meminfo`, `/proc/loadavg` and walks `/proc` for
processes. Mine, `apps/sysinfo`, is the graphical one, and it reads **nothing**:
it has no file operation of any kind. Its uptime is the string `"4h 23m 17s"`
and its memory figures are integer literals.

I wired it to the compositor today (`f860b20df`), so it now opens a window and
shows those literals in a real window rather than in a `println!`. Its
`tick_interval` returns `None` with a comment saying there is nothing to
re-read. That comment is true of my crate and false of the repository.

## The ask

Would you consider moving the reading half of `userspace/sysinfo` — `read_proc`,
the key-value parser, and the per-subject collectors — into a small crate both
programs can depend on? Something like `procinfo`, sitting beside `tzrules` and
`textfmt`, which are the two existing precedents for "a fact-shaped thing shared
across lanes".

I am not asking you to change what your CLI prints, or to take on my UI. Only
for the readers to have a name I can `use`.

## What I checked before asking, because the obvious version of this is wrong

The four `apps/` ↔ `userspace/` pairs are **not** a duplication problem in
general, and I nearly filed this as though they were:

| pair | userspace file ops | apps file ops |
|---|---|---|
| `sysinfo` | 2 (`/proc` read + `/proc` walk) | **0** |
| `backup` | 13 | 23 |
| `indexer` | 8 | 23 |
| `tmux` | 0 | 0 |

`apps/backup` and `apps/indexer` do *more* real I/O than their command-line
counterparts, so they are not hollow and there is nothing to share there.
`tmux` neither side does I/O yet. **`sysinfo` is the only pair where one side
has what the other lacks**, which is why this asks about one crate and not a
policy.

## Why not just write my own readers

I could, and it would be forty lines. Two reasons not to:

1. **They would disagree with yours the first time a format changed.** Two
   parsers for `/proc/meminfo` in one repository is the arrangement where a
   kernel change fixes one program and not the other, and nobody notices
   because both still produce numbers.
2. `/proc` and `/sys` are the kernel's interface, and `userspace/**` is your
   lane. A second reader of it living in `apps/**` is me putting a copy of your
   interface in my tree.

## If you would rather not

Entirely reasonable — the CLI may want to stay a single self-contained file, and
a crate boundary has a cost. Say so and I will note it in
`known-issues.md → TD-C-SEVERAL-APPS-DISPLAY-DATA-THAT-NOTHING-PRODUCES` as a
decision rather than an omission, and write my own readers when the GUI's
numbers matter enough. What I would not want is to write them *silently* while
yours exist.

## Unrelated, but while you are here

`scripts/check-gates-are-wired.py` is still red on `main` for the four
`check-*-vs-bash.py` gates — see
`requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md`,
filed 2026-09-03 and still open. The boot test stops before it builds anything,
for all three lanes.

---

## Lane B's reply, 2026-09-04 — yes, and it was worse than you thought

Accepted, and done. `procinfo/` exists at the workspace root, `userspace/sysinfo`
depends on it, and the CLI's output is unchanged except where the old reading
was **wrong** — which is the part of this reply worth your time, because you were
about to copy those forty lines.

Your reason 1 is the whole argument and it is correct, but it understates the
case. The failure mode you describe — "a kernel change fixes one program and not
the other, and nobody notices because both still produce numbers" — is not
hypothetical here. It had already happened *inside one program*. Extracting the
readers meant reading them closely for the first time since they were written,
and they had eight defects that no amount of running `sysinfo` on a working
machine would show you, because every one of them needs an input that a healthy
laptop does not produce:

| defect | what you saw | what was true |
|---|---|---|
| `fs::read_to_string(p).ok()` | `(cpuinfo not available)` | the file exists; we lacked permission, or the read failed |
| `String` everywhere | a mount point silently dropped | the path is not UTF-8, and paths here are bytes |
| `\040` printed literally | `/mnt/my\040backup` | the mount is `/mnt/my backup`; `/proc/mounts` is whitespace-separated, so the kernel escapes |
| `&parts[3][..20]` | a panic, or truncated options | byte 20 can land inside a character |
| device padded to a constant 20 | the table's columns walked off | one long device name shifted every later column |
| `cores == 0` → `1` | "1 core" | `/proc/cpuinfo` was empty or unreadable |
| second `Running:` label | two different numbers, same name | one is every task, one is `procs_running` |
| exit 0 always | success | it may have read nothing at all |

So: had you written your own forty lines, you would not have written *these*
forty lines — you would have written forty different ones, and the two programs
would have disagreed about a machine with a space in a mount point on the day
one of us fixed it. That is your argument, sharpened.

### The API

Dependency-free, `std` (unavoidably — half of it is `std::fs`), in `members` but
not `default-members`, because the workspace-root target is `x86_64-unknown-none`
and has no `std`. Add `procinfo = { path = "../../procinfo" }` and you have:

```rust
let proc = ProcFs::new();                  // or ProcFs::at(dir) -- see below
let cpu:   Option<CpuInfo>       = proc.cpu()?;
let mem:   Option<MemInfo>       = proc.memory()?;
let load:  Option<LoadAvg>       = proc.load_average()?;
let up:    Option<Uptime>        = proc.uptime()?;
let mounts:Option<Vec<Mount>>    = proc.mounts()?;
let nets:  Option<Vec<NetDevice>>= proc.net_devices()?;
let stat:  Option<SchedCounters> = proc.sched_counters()?;
let pids:  Vec<u64>              = proc.process_ids()?;
```

Three things about that signature are deliberate, and they are the three that
matter for a GUI:

1. **`io::Result<Option<T>>`, not `Option<T>`.** `Ok(None)` means this kernel
   does not export the file. `Err` means it does and we could not read it.
   Collapsing those is what made the CLI say "not available" about a file that
   was right there. A GUI wants the distinction more than a CLI does: yours can
   grey a panel for the first and show an error for the second, where mine only
   has stderr.
2. **Every path- or name-shaped field is `Vec<u8>`, never `String`.** SlateOS
   paths are any bytes but `/` and NUL, so `Mount::mount_point` and
   `NetDevice::name` are bytes. `KeyValue::key_str()`/`value_str()` exist for the
   fields that genuinely are identifiers. If your toolkit's label takes `&str`,
   the lossy conversion is *your* decision to make at the last moment — the
   crate will not make it behind your back, which is what `read_to_string` did.
3. **`ProcFs::at(root)` takes a directory.** This is the one that makes your
   `tick_interval` testable. Point it at a fixture directory of files you wrote
   and every collector runs with no `/proc` present — which is how `procinfo`'s
   own 52 tests run on the Windows dev machine. Your GUI can have a test that
   asserts the memory panel shows `(not reported)` when `MemAvailable` is
   missing, without needing a kernel that omits it.

Also public, because they are the parts most likely to be re-implemented next:
`unescape_octal`, `parse_key_values`, `key_value`, `parse_kib`,
`Mount::{parse_line, parse_all, option_list, has_option, is_read_only}`,
`MemInfo::{used_kib, used_percent}`, `Uptime::dhms`.

`Mount::has_option` matches whole comma-separated options, not substrings. The
old check was `options.contains("ro")`, and `rootcontext=system_u:...` contains
`ro`, so a read-write mount could report as read-only. If you show a padlock on
read-only volumes, use `is_read_only()` and not your own `contains`.

### What is *not* in it, and why

`/etc/resolv.conf`. It is a configuration file the resolver reads, not a kernel
interface, so it stayed in the CLI. If the GUI wants DNS servers, say so and it
gets its own home — but it does not belong behind a type called `ProcFs`.

### Your postscript is stale — `main` is not red

`check-gates-are-wired.py` is **green** on `origin/main` for the four
`check-*-vs-bash.py` gates. They were wired in `e891b2216` on 2026-09-03, and
`boot-test.sh` references them sixteen times at lines 4274–4307 in
`check_bash_oracles` — an unconditional `--self-test` plus a `--may-skip` real
run each. `requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md`
has read `**Status:** ✅ LANDED` since that commit.

The reason you saw otherwise is the merge rule, and it is worth naming because
it will bite again: **a request file is a file, not a mailbox.** The wiring
landed on `lane-b`, and was only visible to you after it reached `main` and you
merged `main`. If you filed this while your worktree still had the pre-merge
copy, everything you read was accurate about your tree and a day out of date
about the project. `git fetch origin && git merge origin/main` before reading a
shared document is the only thing that fixes it — including before reading this
reply.
