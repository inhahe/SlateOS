## §284 — The system's name and version are defined once and read five times, rather than written five times and reconciled

**Date:** 2026-08-22
**Decided by:** Claude (autonomous) — implementing lane B's
`requests/b-a-sysfs-says-mintos-while-procfs-says-linux.md`, whose suggested fix
("ideally by calling into one shared helper rather than by copying the literals
a third time") is the shape adopted here.
**Lane:** A

**In short:** the kernel answers the question "what system is this?" through
five different doors — the `uname` system call, two virtual files under
`/proc`, two under `/sys`, a pair of tunable parameters, and the built-in
shell's `uname` command. Each door had its own copy of the answer written into
it by hand, and they had drifted apart: three different names (`Linux`,
`MintOS`) and four different version numbers (`6.6.0-slateos`, `0.1.0-dev`,
`0.1.0`) were all being reported by the same kernel at the same moment. The fix
is not to correct the copies — it is to delete them, so that there is one
definition and five readers of it.

### What the five doors were saying

| Door | Name | Version |
|---|---|---|
| `uname(2)` | `Linux` | `6.6.0-slateos` |
| `/proc/sys/kernel/{ostype,osrelease,version}` | `Linux` | `6.6.0-slateos` |
| `/sys/kernel/{ostype,osrelease,version}` | **`MintOS`** | **`0.1.0-dev`** |
| `kernel.ostype` / `kernel.osrelease` sysctl | **`MintOS`** | **`0.1.0`** |
| `/proc/version` | **`MintOS kernel 0.1.0 (Rust, x86_64, 16 KiB pages)`** | — |
| kshell `uname -sr` | **`MintOS`** | **`0.1.0`** |

Lane B reported the third row. The fourth, fifth and sixth were found while
fixing it, and are the reason the fix is a module rather than three edited
literals: three known-wrong copies out of six is not a typo, it is what copying
does over time. Lane B had already found the identical shape on its own side —
four userspace components each holding a private answer — and fixed it the same
way (read the kernel, keep no independent value).

### Why `Linux` is the honest answer here and not a lie

Settled policy, user-directed 2026-06-10, recorded as `roadmap-detailed.md` §72
"Version-surface policy". `uname(2)` is a Linux system call, and `/proc` and
`/sys` are Linux inventions; native SlateOS code uses native APIs and never
reads any of them. So the only programs that can see these strings are Linux
binaries asking which Linux personality they are talking to, and `Linux` is the
true answer to *that* question. The product name is not suppressed — it moves to
the one field that is actually about the product (below).

`osrelease` is the field with teeth, and it is why this was worth fixing before
anything read the wrong copy. glibc's `__libc_start_main` parses the leading
integer triple of the release string and aborts the process with
`FATAL: kernel too old` if it is below the minimum glibc was built against.
`6.6.0-slateos` clears that gate; `0.1.0-dev` does not. A future consumer
picking the `/sys` copy over the `/proc` copy — a coin-flip for anyone who does
not know this history — would have produced "this ported program aborts at
start-up for no visible reason", which is close to the worst
symptom-to-cause distance in the tree.

### Where the product name belongs: `uname -o`, and one word

`OPERATING_SYSTEM = "SlateOS"` is the sixth constant, and it is *not* a
`struct utsname` field on Linux either — GNU's `uname -o` is a compile-time
constant, which is exactly how GNU reports `Linux` for `-s` and `GNU/Linux` for
`-o` without contradicting itself. That distinction is what makes the whole
change non-lossy: the kernel still says what it is, in the field whose question
is "what operating system is this?" rather than the one whose question is "what
kernel ABI am I calling?".

It is spelled without a space deliberately. `uname -a` prints the fields
space-separated and callers routinely split it on whitespace, so a two-word
value silently becomes two fields and shifts everything after it. Lane B's
`userspace/coreutils/src/bin/uname.rs` reached the same spelling independently
and for the same reason (its comment notes `Slate OS` "had a space in it"); the
kernel constant matches it byte for byte so the in-kernel shell's `uname` and
the ring-3 one cannot disagree.

### `/proc/version` was reformatted, not just re-valued

Linux builds `/proc/version` from `struct utsname` with a fixed banner
(`fs/proc/version.c` → `linux_proc_banner`):

```text
%s version %s (COMPILE_BY@COMPILE_HOST) (COMPILER) %s\n
 ^sysname    ^release                              ^version
```

Ours read `MintOS kernel 0.1.0 (Rust, x86_64, 16 KiB pages)`. The *shape* was
wrong, not only the values: `Linux version (\S+)` — the regex every version
sniffer uses — matched nothing, and a caller falling back to positional parsing
got `0.1.0`. That is not hypothetical; our own `userspace/lsmod` takes the third
whitespace token of `/proc/version`, which is where Linux puts the release. It
happened to work only because `MintOS kernel 0.1.0` puts a version in slot three
too — the right slot by accident, with the wrong value in it. The banner form
fixes both at once and needs no change in lsmod.

**Alternative considered: keep the descriptive text.** `(Rust, x86_64, 16 KiB
pages)` is genuinely informative and appending it after the version field would
be harmless to both parsing strategies, since Linux's own version field contains
spaces and runs to the end of the line. Rejected because "harmless to the two
parsers I thought of" is a weaker property than "identical in shape to the thing
every parser was written against", and the 16 KiB page size is already reported
where a program would look for it.

### What was deliberately *not* touched

The kernel says `MintOS` in about twenty other places — boot banners, the login
screen's welcome message, the HTTP `Server:` header, LLDP system descriptions,
the TLS self-signed certificate's CN, installer disk labels. None of those is an
ABI surface answering "what system is this?"; they are branding, and branding is
allowed to be a display name. The rule this entry establishes is narrow on
purpose: *the doors that answer the same question must give the same answer*.
Sweeping every occurrence of a product name into one constant would be a
different and much less defensible change, because it would couple a window
title to a glibc start-up gate.

### The test that had to change, and why the old one was the problem

`sysfs::self_test` asserted `ostype.starts_with(b"MintOS")`. That assertion is
why the bug survived: a test written against one generator's own literal can
only ever agree with it, so it certified the divergence rather than catching it.

The replacement compares **generated bytes against generated bytes** — it
constructs a `ProcFs` alongside the `SysFs` and reads the same three names out of
both, requiring the two outputs and `crate::uname` to agree. That catches a
drifting literal *and* a generator that quietly stops reading the shared module,
which asserting the constant could not. `procfs::self_test` was likewise
re-pointed at the constants rather than at a fourth copy of the strings.

`uname::self_test` deliberately does **not** assert that the constants equal
themselves — that passes by construction and would be a fifth copy. It checks
the two properties a caller can actually be broken by: that `RELEASE` parses to
at least 6.6 the way glibc parses it, and that every field except `VERSION` is a
single whitespace-free token.
