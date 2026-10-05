### HOST-OILS-MSYS-DERIVES-THE-EXECUTE-BIT-FROM-A-SHEBANG. `[ -x file ]` disagrees on the Windows dev host — WONTFIX (host artifact) — 2026-08-04

**Not an osh bug.** MSYS's `access(X_OK)` answers *yes* for a file whose first
two bytes are `#!`, whatever the filesystem says, because Windows has no execute
bit and MSYS synthesises one. Rust's `std::fs` does not, so:

```sh
printf '#!/bin/sh\necho x\n' > bin/tool
[ -x bin/tool ]; echo $?
```

is `0` under the reference bash and `1` under osh, with no `$EXECIGNORE`
involved. The same asymmetry is why the `$EXECIGNORE` corpus case
(`execignore-makes-a-file-stop-counting-as-executable.sh`) deliberately omits
`test -x` / `[[ -x ]]` even though bash's answer there is interesting (they are
*not* filtered by `$EXECIGNORE`).

On SlateOS there is a real execute bit and the two agree by construction, so
there is nothing to fix — recorded only so the next investigation does not
mistake it for a divergence in the code under test.
