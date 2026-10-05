## 850. Hardware facts are served under `/sys/devices`, not a second `/sys/hardware` tree

**Date:** 2026-09-14
**Lane:** C
**Decided by:** Claude (autonomous) — lane A proposed the option and withdrew its
own first choice; lane C made the final call, because the code that changes is
lane C's.

**In short:** the System Information program reads hardware details from files
the kernel is supposed to publish. Nobody had ever built the kernel half, and
when we came to build it we found the kernel *already* publishes some of the
same facts somewhere else, under a different name. The choice was to build the
second name anyway — cheap for lane C, no code changes — or to use the one that
exists and rewrite the reader. We are using the one that exists.

**The two names.** `apps/sysinfo/src/hwquery.rs` reads `/sys/hardware/cpu`,
`/sys/hardware/memory` and ten more, each a single file of `key=value` lines.
`kernel/src/fs/sysfs.rs` serves `/sys/devices/pci/BB:DD.F` and
`/sys/devices/system/cpu/cpuN/...` — the Linux shape, one scalar per file, a
directory per device. `design.txt` mentions neither path, so there was no
authority to appeal to.

**Why the existing tree wins, and it is not mainly about names.** The kernel
already publishes `core_id`, `physical_package_id`, `online`, `possible`,
`present` and cache `size`/`level`/`type`. `hwquery`'s `physical_cores` and
`logical_processors` are derivable from those, and its `l1_data_kb`/`l2_kb`/
`l3_kb` from the cache indices. Building `/sys/hardware` too would have the
kernel publish **the same facts twice, in two layouts** — a core count as a
scalar under one tree and as a `key=value` line under the other. That is two
models of one fact at the level of the operating system's own interface, and it
would be permanent: a tree is much harder to withdraw than a file.

Matching Linux is the secondary reason and still a real one. This project ports
battle-tested code elsewhere for the same reason, and a sysfs that is shaped
like Linux's is one that ported tools can read.

**What it costs, stated plainly because the first version of this decision had
it wrong.** Lane A's table put lane C's cost at "one literal", on the strength
of the `sysfs!` macro introduced the same afternoon which collapsed twelve path
constants into one base. That is true of the *paths* and false of the *parser*:
the two trees differ in data model, not just name, so `hwquery.rs` needs a
reader that walks a directory tree instead of one that splits `key=value`
lines. Lane C confirmed this by reading `sysfs.rs` rather than accepting the
table, and chose the option that costs lane C more.

**What remains to build either way:** the CPUID-only facts (`family`, `model`,
`stepping`, base and turbo clocks) and the memory and display facts exist in
neither tree. They go under `/sys/devices/system/...` in the same scalar-per-file
shape.

**The contract that carries over unchanged:** the kernel emits only what it can
honestly answer and **omits** the rest. It never writes `0` for unknown. A
missing file is a missing fact and the reader's default stands; a file that is
present and will not parse is an error. Scalar-per-file makes that easier to
honour than a `key=value` blob did — an absent fact is simply an absent file.

**How it was nearly decided the other way.** The question sat blocked for ten
days between two request files, each of which recorded that it was waiting for
the other, which in either file alone reads exactly like being correctly
blocked. It was then nearly settled as `/sys/hardware` on the strength of a
grep for `sys/hardware` in `kernel/src` returning zero — which meant "no
producer under that name", not "no producer". Both failures are the same one
this file keeps recording: a true answer to a narrower question than the one
being asked.
