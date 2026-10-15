## TD-B-PKG-COMMANDS-HAVE-NO-TESTS (lane B, 2026-10-15)

**Status:** OPEN

**In short:** `pkg`'s commands -- `main`'s dispatch, `pkg pack`, `pkg info`,
`pkg search --font`, the usage text -- are run by no test. `pkg` keeps its
state at fixed paths (`/var/pkg`, `/etc/pkg.conf`) and installs files at
the absolute paths its packages name, so a test that ran a command would
read and write the real system's package database. Its unit tests cover
the functions underneath; what each command does with them -- which it
calls, what it prints, the status it exits with -- is checked by nothing.

**Found by:** mutation testing in the review of 8f24de7ed..8e021f7f9
(`20261015T103307-8e021f7f9808-mutants`). Six mutants survived because of
it: `main` and `cmd_pack`, `cmd_info`, `cmd_search_font` and `print_usage`
each replaced by nothing, and the `search` arm deleted from `main`. Removing
any of them leaves every test green.

**Where:** `userspace/pkg/src/main.rs` -- the `PKG_ROOT`, `CAS_DIR`,
`GEN_DIR`, `REPO_DIR` and `CONFIG_PATH` constants, `PackageDb::new`,
`PkgConfig::load`, and every `cmd_*`.

**The proper fix:** give `pkg` a root -- `pkg --root DIR`, under which its
state, its configuration and the files it installs all live, as `dpkg
--root`/`rpm --root` have it. An installer needs it anyway, to install into
a system that is not the running one. With it, command tests run the built
binary (`CARGO_BIN_EXE_pkg`) against a scratch root: pack a package from a
fixture, install it from a local repository index, and check `info`,
`search`, `search --font`, `list` and `remove` by their output and status.
