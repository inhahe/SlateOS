## 1064. `-Z` and `--context` are taken as GNU takes them on a kernel without SELinux

**Date:** 2026-10-07
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** several GNU tools that make files (`mkdir`, `mkfifo`, `mknod`,
`install`, `cp`, `mv`) accept `-Z` or `--context`, which label the new file for
SELinux (a Linux security system SlateOS does not have). On a Linux without
SELinux, GNU's tools accept `-Z` and do nothing, and when a specific label is
named they print a one-line warning and carry on. Our `mkdir`, `mkfifo` and
`mknod` refused both outright with an error, so a script that passes `-Z`
everywhere -- and works on every Linux without SELinux -- failed here. This
decides that they behave as GNU does on such a kernel.

### What it is

| command | GNU 9.4 on a kernel without SELinux or SMACK | was here |
|---|---|---|
| `mkdir -Z d` | makes `d`, silent, status 0 | `option -Z is not implemented`, status 1 |
| `mkdir --context d` | the same | refused the same way |
| `mkdir --context=CTX d` | `mkdir: warning: ignoring --context; it requires an SELinux/SMACK-enabled kernel`, then makes `d`, status 0 | refused |
| `mkdir --context= d` | the same warning: an empty value is still a value | refused |

The warning is printed from inside the option loop, so it comes before a
`missing operand` or an invalid option that follows it. Measured by
`scripts/mkdir-diff.sh` against coreutils 9.4 built from source.

`install` already did this (`install.rs`, "SELinux"), with its own wording --
`it requires an SELinux-enabled kernel`, no SMACK. `mkdir`, `mkfifo` and
`mknod` share the sentence above word for word (`mkdir.c`, `mkfifo.c` and
`mknod.c` in 9.4 carry the same `case 'Z'`), and all three take it from this
change, each held to GNU by its harness.

### Why

The refusal rested on one argument, in the three modules' docs: "silently
dropping a requested security context is the defect". It does not survive
looking at what upstream does. Nothing is dropped silently: a context the user
*named* is warned about. What `-Z` alone asks for is "the default context",
which on a kernel with no security contexts is no context at all -- there is
nothing to drop. And upstream's behaviour is not an accident of one tool: it
is the same in every tool that has the option, because scripts and package
recipes pass `-Z` unconditionally and rely on it being harmless where SELinux
is absent.

### The alternative, and what it would cost

**Keep refusing.** A user who really needed a label would learn at once that
none was applied, rather than from a warning they might not read. But that
user is on the wrong system either way -- SlateOS has no SELinux to label
for -- and the price is paid by everyone else: every script that passes `-Z`
portably stops at the first `mkdir`. GNU's own warning already tells the one
user who named a label. If SlateOS ever gains a labelling security module,
this is revisited with it: the option would then have something to do.
