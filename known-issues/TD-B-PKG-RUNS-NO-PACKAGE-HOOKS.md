## TD-B-PKG-RUNS-NO-PACKAGE-HOOKS (lane B, 2026-10-16)

**Status:** OPEN

**In short:** a package can name scripts to run before and after it is
installed or removed (`hook-pre-install:` and the rest), and `pkg` reads
them, but no command runs them. A package that needs one -- to make a user,
to rebuild a cache -- installs without it, and nothing says so.

**Found by:** the Opus review of 0dadd1110..9454a6359 (finding 14), which
noticed that `pkg --root`'s help text promised "Hooks run inside it" while
the code that would run them is reached by nothing. The help text no longer
says it (2026-10-16).

**Where:** `userspace/pkg/src/main.rs`, `PackageHooks` (`#[allow(dead_code)]`).
Its `run` already chroots into `--root`'s directory before it executes a
hook (an `unsafe` `pre_exec` block whose safety argument is that `pkg` is
single-threaded); that block has never run under a test either.

**The proper fix:** call `PackageHooks::run` from install, upgrade and
remove at the four points the manifest names, abort the operation when a
pre-hook fails (as the doc comment says), and test each with a package
whose hooks write a file under a scratch `--root`: that the file appears
inside the root, in the order the operation reaches each hook, and that a
failing pre-hook leaves the package uninstalled.
