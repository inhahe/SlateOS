### [A] 340 of 430 `kernel/src/fs` modules have no consumer but `/proc` -- and for a microkernel that is mostly right. The defect is what their docs say -- 2026-09-17

**Status:** OPEN

**In short:** most of the kernel's "feature" modules keep a setting, show it
in `/proc`, and do nothing else. That sounds alarming and mostly is not: this
is a microkernel, so the *doing* is supposed to happen in userspace. What is
wrong is that each module's opening comment describes itself as performing
the feature -- "Provides facial recognition ... authentication" -- when what
it provides is a table for something else to act on. A reader believes the
feature exists.

**The measurement.** For each of the 430 `kernel/src/fs/*.rs`: does it touch
hardware (port I/O, MMIO, `pci::`, MSRs), and does anything reference
`<name>::` other than itself, `procfs.rs`, `kshell.rs`, or a `self_test`
dispatch? **340 have neither.**

**Two of my own measurements disagreed first, and the wrong one was the new
one.** A per-module `grep` said `faceunlock` had no consumers; a tree-wide
pass said only 3 of 430 were unwired. The tree-wide pass counted
`X::self_test()` in `main.rs` as a consumer -- and nearly every `fs` module
has one, so "has a self-test" was being read as "is used". The verdict was
computed over a definition of *used* that included *tested*, which is dd-942
in an instrument three minutes old. Excluding self-test dispatch moved the
answer from 3 to 340.

**Validated against cases I could reason about independently**, because a
number that large deserves a check that does not come from the same script:

| module | every external reference |
|---|---|
| `binfmt` | 3 in `procfs.rs`, 3 in `kshell.rs`. See the correction below -- my first reading of this one was wrong. |
| `brightness` | `procfs.rs` stats, and `kshell.rs:80881 brightness::init_defaults()`. No backlight is ever set. |
| `faceunlock` | 2 in `procfs.rs`. Nothing else. |

**Why this is mostly not a defect.** `CLAUDE.md`'s first architectural rule:
*"Microkernel: drivers run in userspace. Only scheduler, memory manager,
IPC, capability enforcement, and interrupt routing run in kernel space."* A
kernel that dimmed displays or ran facial recognition would be violating
that. A `/proc`-exposed state store that a userspace service acts on is the
shape the design asks for.

**Why it is still a defect.** The module docs are written as though the
kernel were monolithic:

| module says | what it is |
|---|---|
| `faceunlock`: "Provides facial recognition enrollment, verification, and authentication" | a table of enrolled templates and a threshold |
| `filetransfer`: "Provides nearby device discovery and file transfer ... using Wi-Fi Direct / Bluetooth" | a table of peers and transfers |
| `tasksched`: "Provides timed execution of commands" | a table of schedules nothing runs |

Each would be true with one word changed -- *records* rather than *provides*
-- and each is currently the claim lane C's
`TD-C-A-MODULE-DOC-IS-THE-ONE-CLAIM-NOTHING-CHECKS` says nothing in the tree
checks. dd-950 applies to the fields inside them: the unread fields in these
modules are the shape of the userspace consumer, not clutter.

**So does the userspace side exist? Measured, and the answer is "once".**
Grepping `userspace/`, `services/`, `apps/`, `gui/` and `posix/` for reads
of these `/proc` files:

| file | external readers |
|---|---|
| `/proc/brightness` | **1** -- `gui/desktop/src/power_settings.rs` |
| `/proc/powerprofile`, `/proc/energysaver`, `/proc/gamemode`, `/proc/faceunlock`, `/proc/tasksched`, `/proc/devpower`, `/proc/binfmt` | **0** each |

That zero is a real result and not a pattern miss, checked the way dd-942
asks: **114** files outside the kernel do read `/proc`, on live paths --
`/proc/sys` 119 times, plus `/proc/net`, `/proc/self`, `/proc/uptime`,
`/proc/meminfo`, `/proc/cpuinfo`. The grep finds readers where readers exist.

**Which makes this constructive rather than damning.** The
kernel-records/userspace-acts design is not hypothetical -- it works, and
`power_settings.rs` reading `/proc/brightness` is the worked example of what
the other modules need. The kernel half is built, one userspace consumer
exists, and the rest of the middle is missing. That is a wiring backlog with
a template, not an architectural error.

Worth noting `/proc/sys` is read 119 times while these bespoke files are read
once between them. Whether new kernel state should surface through sysctl
rather than a per-feature `/proc` file is a real design question and not
mine; it would at least put new state on the path userspace already uses.

**What remains above my lane.** Lane C found the mirror image one layer up:
`services/netstack` exists and no app uses it, and six `apps/` crates depend
only on `appearance`/`guitk`/`oswindow` with no file or socket access at all.
Kernel tables with no readers and apps with no I/O are two halves of the same
gap, and closing it is an architectural programme rather than a bug fix.

**Scope note, so this is not read as 340 defects.** Three individual cases
were fixed today where the claim was checkable and the disclosure cheap:
`devpower`, `syshealth`, and the `/proc` readers for `powerprofile`,
`energysaver` and `gamemode`. The remaining ~337 are a documentation pattern,
not 337 bugs, and a sweep rewriting 337 module docs on one agent's reading of
the architecture would be exactly the kind of unilateral change dd-951 is
about.

### Correction 2026-09-17: `binfmt` is a statistics module, and I called it a registry

The row above originally read *"nothing in the exec path consults it, so the
binary-format registry is decorative"*. That was wrong, and it reached `main`
before I read the module's first line:

> `//! Binary Format -- executable format loader statistics.`

`binfmt` is not something `exec` should *consult*. `register_format`,
`record_load` and `record_error` are recorders: the module's job is to be
**told** what the loader did. So the question is the reverse of the one I
asked -- and the answer is still a gap, just a smaller and different one.
**Nothing reports to it.** Outside `binfmt.rs` there is not one call to
`record_load`, `record_error` or `register_format`; all 11 references are its
own API and self-test. `/proc/binfmt`'s load and error counts are therefore
permanently zero, and a reader would conclude no binary has ever been
executed on this system.

That is dd-946 from the other side: not a publisher with no subscriber, but a
**subscriber with no publisher**. The fix is one call in the ELF loader, and
it is a real instance of the wiring backlog this entry describes rather than
a documentation defect.

Recorded as a correction rather than a silent edit, because the wrong version
is already on `main` and because the mistake is instructive: I inferred a
module's purpose from its **name** and its function signatures without
reading its first line. dd-947's rule is to grep the tree for prose about the
thing before instrumenting it, and the prose here was line 1 of the file I
already had open.
