## 758. `/proc` gets a crate of its own, and its readers return "not exported" and "could not read" as two different answers

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** Two programs in this tree call themselves `sysinfo`. The
command-line one read the kernel's `/proc` files; the graphical one showed made-up
numbers typed into the source. Lane C asked whether the reading half could become
a shared crate so both show the same machine. It now is — `procinfo` — and while
moving the code we found that it had been answering several questions wrongly for
as long as it had existed. The visible change for a user: a mount point with a
space in it prints correctly, a directory listing no longer gets cut off in the
middle, and `sysinfo` now exits with an error code when it could not read
something, instead of always claiming success.

### The decision

Three separate calls, taken together because each depends on the one before.

**1. The readers move to a crate rather than being copied.** `procinfo/` sits at
the workspace root beside `textfmt` and `yamldoc`, dependency-free, and
`userspace/sysinfo` is now formatting only. Filed as
`requests/c-b-the-proc-readers-in-userspace-sysinfo-should-be-a-crate-both-sysinfos-can-use.md`.

*Alternative:* lane C writes its own forty lines. Cheaper today, and the request
itself named why not: two parsers of `/proc/meminfo` in one repository is the
arrangement where a format change fixes one program and not the other, and nobody
notices because both still produce numbers. The extraction proved the point
harder than the argument did — see below.

*Cost paid:* a crate boundary, and a `std` crate in a workspace whose root target
(`x86_64-unknown-none`) has no `std`. It is in `members` — so it inherits
`[lints] workspace = true` and shares one `target/` — and out of
`default-members`, so a bare `cargo build` at the root does not try to build it
for a target with no standard library. That split is the same one the other host
crates use.

**2. Every reader returns `io::Result<Option<T>>`, not `Option<T>`.** `Ok(None)`
means *this kernel does not export that file*. `Err` means it does and we could
not read it. Only `io::ErrorKind::NotFound` is allowed to become `Ok(None)`.

*Alternative:* keep `fs::read_to_string(path).ok()`, which is one line and reads
nicely. It conflates a file that is absent with a file we were denied, and the
program printed `(cpuinfo not available)` for both. The stronger form of the
objection is that the old shape made the *caller's* correct behaviour
unexpressible: `sysinfo` exited 0 whether it had described the machine or printed
six "not available" lines, because it had no way to know the difference. It now
exits 1 on a real read error. A system-information tool that cannot tell you it
failed is worse than one that fails.

*Cost paid:* every call site gains a `?` or an explicit match, and the CLI grew a
`Status` type to thread the distinction to its exit code. That is the honest
price of the distinction existing at all.

**3. Every path- and name-shaped field is `Vec<u8>`, never `String`.** SlateOS
paths are any bytes except `/` and NUL (`design.txt`), so `Mount::mount_point`,
`Mount::device` and `NetDevice::name` are bytes end to end, and the CLI formats
through a byte-buffer `Row` rather than `println!`.

*Alternative:* `String` with `from_utf8_lossy`, which is what the old code did
implicitly. It is not merely lossy in the abstract: it replaces a run of bytes
with a single U+FFFD, so a mount at a non-UTF-8 path came out unusable, and two
different mounts could print identically.

*Cost paid:* the formatting half cannot use `println!` for values, and column
widths must be computed (characters when the bytes are UTF-8, bytes when they are
not) rather than taken from `str::len`. Both are contained in about thirty lines
of `Row`/`column_width`, and both are tested.

### Why the extraction was worth more than the sharing

The request asked for a crate. Reading the code closely enough to move it found
eight defects, none of which a run on a working machine would reveal, because each
needs an input a healthy laptop does not produce:

| defect | showed as | truth |
|---|---|---|
| `read_to_string(p).ok()` | "not available" | the file exists; the read failed |
| `String` for paths | a mount silently mangled | the path is not UTF-8 |
| `\040` printed literally | `/mnt/my\040backup` | `/proc/mounts` is whitespace-separated, so the kernel escapes space, tab, newline and backslash as octal (Linux `fs/proc_namespace.c`) |
| `&parts[3][..20]` | a panic, or cut options | byte 20 can land inside a character |
| device padded to a constant 20 | the table's columns walked off | one long device name shifted every later column |
| `cores == 0` → `1` | "1 core" | `/proc/cpuinfo` was empty or unreadable |
| `options.contains("ro")` | a read-write mount shown read-only | `ro` is a substring of `rootcontext=…`; options must match whole |
| two `Running:` labels | one number overwriting another | one counts every task, one is `procs_running` |
| exit 0 always | success | it may have read nothing |

The general lesson, which is the reason this is a design decision and not a
changelog entry: **code that only ever runs against a healthy system is untested
code, however often it runs.** The fix that makes it testable is the one API
choice not listed above — `ProcFs::at(root)` takes a directory, so every collector
runs against a fixture of files we wrote, on a host with no `/proc` at all. That
is how `procinfo`'s 52 tests run on the Windows dev machine, and it is what let
the eight defects be pinned by tests rather than by argument.

### What stayed out

`/etc/resolv.conf` is read by the CLI and not by `procinfo`. It is a
configuration file the resolver reads, not a kernel interface, and a type called
`ProcFs` that reads `/etc` is a type whose name has stopped being true.
