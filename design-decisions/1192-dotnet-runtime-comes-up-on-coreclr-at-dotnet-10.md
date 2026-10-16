## 1192. dotnet/runtime comes up on CoreCLR, at .NET 10

**Date:** 2026-10-16
**Decided by:** Claude (operator-approved scope: the operator chose dotnet/runtime over Mono 6.14, §1246; which of its runtimes and which release are lane D's call)
**Lane:** D

**In short:** the operator changed the .NET port from Mono 6.14 to
dotnet/runtime, the modern .NET that runs programs written for .NET 8 and
later (§1246). dotnet/runtime has two runtimes: CoreCLR, the one Microsoft
ships for Linux and Windows, and its own Mono, used mostly on phones and in
browsers. The first one brought up here is CoreCLR. Its garbage collector
stops each thread by sending a signal to that thread alone, which SlateOS can
do since lane A made signals per-thread (§1542). The release is .NET 10, the
current long-term-support one; a program written for .NET 8 runs on it when
told to roll forward to a newer major version.

**The choice of runtime:**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. CoreCLR first** (chosen) | the runtime .NET programs are built and tested on runs here, with its optimising JIT | the most compatible and the fastest; what the operator's programs were written against | asks most of the C library: a signal aimed at one thread with a context its handler rewrites (GC suspension; faults become exceptions), code mapped twice -- writable and executable -- for its W^X policy (or that policy turned off), and its own unwinder |
| B. dotnet/runtime's Mono first | the .NET 10 libraries run on a simpler runtime that suspends threads cooperatively | carries over the Mono 6.14 spike's work (cooperative suspend, faults rewritten into exceptions, `dlsym` for P/Invoke) | a slower JIT; on desktop Linux it is a configuration mostly tested by its own maintainers, not the one programs meet |

B stays the fallback if CoreCLR meets a wall the library cannot pass in
reasonable time: the class libraries are the same, so nothing above the
runtime is lost.

**The choice of release:** .NET 10 (long-term support until November 2028),
not .NET 8, whose support ends in November 2026. A program built for .NET 8
runs on the .NET 10 runtime with `DOTNET_ROLL_FORWARD=Major` (or
`"rollForward": "Major"` in its runtime configuration), which `/bin/dotnet`'s
wrapper on the image sets.

**What happens to Mono 6.14.** Its spike (`scripts/mono-spike/`) stays until
CoreCLR runs here, as the measure of what the C library already gives a .NET
runtime; then it is removed, with `/bin/mono` and `services/ctest-mono-runs`.
