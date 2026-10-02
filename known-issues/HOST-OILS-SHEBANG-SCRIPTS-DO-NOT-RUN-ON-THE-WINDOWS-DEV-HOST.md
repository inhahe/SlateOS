### HOST-OILS-SHEBANG-SCRIPTS-DO-NOT-RUN-ON-THE-WINDOWS-DEV-HOST. `#!/bin/sh` names a file the host does not have — WONTFIX (host artifact) — 2026-08-04

**Not an osh bug.** A corpus fixture written as `#!/bin/sh` runs under the
reference bash on this host because MSYS resolves `/bin/sh` inside its own
mounted root; osh keeps native paths, so `/bin/sh` names nothing and the spawn
fails. osh's own shebang handling (`shell_script_indirection`) is correct and is
tested independently — it is the *interpreter path in the fixture* that is
unresolvable, not the mechanism.

**Consequence for corpus cases.** A case that needs an external program to
actually *run* cannot get one by writing a `#!` script, so
`a-path-entry-is-reported-exactly-as-it-was-written.sh` and
`execignore-makes-a-file-stop-counting-as-executable.sh` confine themselves to
*describing* such files (`command -v`, `type`) plus one shell-source fixture for
the running half. Nothing to fix; recorded so the constraint is not
re-discovered.
