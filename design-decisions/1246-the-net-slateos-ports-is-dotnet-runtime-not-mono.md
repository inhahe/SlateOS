## 1246. The .NET SlateOS ports is `dotnet/runtime`, not Mono

**Date:** 2026-10-10 · **Decided by:** Operator (in chat to lane E's session,
answering lane E's finding below; Claude proposed `dotnet/runtime`). The
operator's words are below and in
`operator-answers/2026-10-10-chat-dotnet-runtime-instead-of-mono.txt`. ·
**Lane:** E (recorded, where the operator answered); the port is lane D's.

**In short:** The operator asked for Mono, "dotnet support for Linux", so
that C# programs run on SlateOS directly, their own among them (§1050).
Lane D has built Mono 6.14. But Mono 6.14 runs programs written for the old
.NET Framework 4, and both of the operator's C# programs are written for
.NET 8, which it cannot load. What runs .NET 8 programs on Linux is current
.NET, whose runtime and libraries are the `dotnet/runtime` repository. The
operator chose it: the port's target changes from Mono to `dotnet/runtime`.

> yes, change mono to dotnet/runtime.
>
> -- the operator, 2026-10-10, answering lane E's report that Mono 6.14
> cannot run their .NET 8 programs

**The finding it answers.** Read from each program's project files on
2026-10-10:

| Program | Target | Window |
|---|---|---|
| LithicBackup, `D:/visual studio projects/backup` (about 50,000 lines of C#) | `net8.0-windows` | WPF, with WinForms |
| NotepadRedo, `D:/visual studio projects/notepad` | `net8.0-windows` | WPF, with WinForms |

Mono 6.14 implements .NET Framework 4 and its class libraries. A .NET 8
program is built against .NET 8's libraries, which Mono 6.14 does not have.
`dotnet/runtime` is the runtime and the libraries of .NET 5 onward, and it
runs on Linux today.

**What it does not change.** WPF and WinForms exist only on Windows, whatever
the runtime, so neither program's window runs on SlateOS as it is. Each
window is rewritten in a toolkit that does run here: Avalonia (itself a port)
or SlateOS's own. LithicBackup also calls Windows services that SlateOS
provides its own way: the NTFS change journal, and IMAPI2 for burning discs.
The operator gave two routes for LithicBackup (§1050): reimplement it, or run
it on .NET. Lane E chooses between them once `dotnet/runtime` runs here
(`roadmap.md`, lane E's backlog).

**For lane D -- not decided here.** `dotnet/runtime` carries two runtimes,
which run the same libraries:

- **CoreCLR**, which .NET normally runs on. To stop a thread for its garbage
  collector, it sends that thread a signal (its "activation" signal). That is
  the per-thread signal handling lane D's Mono port went without: lane D
  built Mono with cooperative suspend because SlateOS has no per-thread
  signal masks.
- **A Mono** that shares its origin with the one lane D has built, which .NET
  uses on Android, iOS and WebAssembly. It can suspend threads cooperatively,
  the same way.

Which of the two to bring up first, and which .NET release, is lane D's call.
The Mono 6.14 work carries over to either: the C library's gaps filled, and
`dlsym` over the runtime's own symbols (§1184).
