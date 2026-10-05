## 1050. Lane B's next large port is the fastpy compiler on SlateOS; the operator's further ports are recorded

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q18 with "Claude's recommendation":
option B, the fastpy compiler, then the bug list as the standing default --
and adding a list of ports). Relayed verbatim through lane F's session.

**In short:** of the three large ports left in lane B's list, the fastpy
compiler comes first: half of it already works, and it is what lets OS
components be written in Python on SlateOS itself. The Rust toolchain and
WINE wait. The operator also named programs they want ported that the
roadmap did not carry; they are now recorded.

**The operator's answer, verbatim:**

> Claude's recommendation, though I noticed that Chromium is missing from
> the list of large things to port. Have you ported that already? Oh, and
> one thing I forgot to mention, I want Mono (dotnet support for Linux)
> ported too, so record that. And record Chromium if it's somehow not
> recorded anymore. Another thing I want ported is Xonsh, and another is
> YSH, and another is Nushell. And QDirStat, or better, port my own fork of
> WinDirStat that can be found at d:\visual studio projects\dirsize\windirstat.
> Also, do we have a capable debugger, like cdb? We should port one or more
> of those, too. And I want a reimplementation of `d:\visual studio
> projects\backup` in a language we support, or if we get Mono ported, we
> can just run it on that. Record all of these that aren't recorded.

**Each item, checked against the roadmap:**

| Port | Recorded before? | Now |
|---|---|---|
| Chromium | yes -- a joint task driven by lane E, blocked on POSIX, GPU and networking; it was absent from B-Q18 only because it is not lane B's | unchanged |
| Mono (.NET on Linux) | no | added |
| Xonsh | no | added |
| YSH | yes, as the half of genuine Oils §73 deferred | it arrives with §1043 |
| Nushell | marked `[x]`, but what was verified is a *Windows* build (`nu.exe` under msvc, 2026-06-03), not SlateOS | added as a SlateOS port, and the `[x]` annotated |
| WinDirStat (the operator's fork), or QDirStat | a WinDirStat-*style* app exists (`apps/diskanalyzer`), written here, not a port | added |
| A capable debugger | a hand-written `gdb` crate exists; no port of GDB or LLDB | added |
| `d:\visual studio projects\backup` (LithicBackup) | no | added: reimplement, or run it on Mono once Mono runs |
