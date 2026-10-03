### TD-OILS-ENVNAME-IMPORT. `osh` imports environment variables whose names are **not valid shell identifiers** (e.g. Windows' `PROGRAMFILES(X86)`) as ordinary shell variables; bash keeps them in the child environment but hides them from the shell-variable namespace — 2026-07-19 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `import_environment` (~633) inserts
every `std::env::vars()` pair into `self.vars` + `self.exported` without checking
the name. Child-env construction (~986, ~3836, ~4191) then re-emits them.

**What:** bash only turns an inherited env var into a *shell variable* when its
name matches `[A-Za-z_][A-Za-z0-9_]*`; names with other characters (parentheses,
dots, etc.) are retained in the process environment and passed through to
children, but are invisible to `set` / `export -p` / `${name}`. osh currently
lists them (they show up in `export -p` and `set`), which diverges from bash.
Only observed on the Windows host, where the inherited environment contains
`PROGRAMFILES(X86)` / `COMMONPROGRAMFILES(X86)`.

**Why deferred:** the *correct* fix is to keep a separate raw-environment store
(e.g. `raw_env: Vec<(OsString, OsString)>`) for invalid-name entries, merge it
into the child environment at every spawn site, and never surface it as a shell
variable — a small refactor touching `import_environment`, the three spawn-env
loops, and `clone_for_subshell`. Simply dropping such names at import would match
bash's *shell-variable* view but stop passing them to children, which could break
host programs that rely on `PROGRAMFILES(X86)`. On SlateOS the inherited
environment will use well-formed identifier names, so this is purely a host-test
artifact; parked until it matters. Impact: low — cosmetic `set`/`export -p`
listing noise during host comparison testing only.

**Fixed 2026-07-31.** The blocker named above — "needs a separate raw-env
passthrough store" — was built for TD-OILS-NONUTF8-ENV-NAME: `Shell::opaque_env`
holds entries verbatim as `OsString` pairs and `apply_child_env` is the single
place a child environment is assembled, so both spawn paths already re-emit them.
A non-identifier name is the same problem as a non-UTF-8 one — an environment
name is any byte sequence without `=`, and neither kind can be written as a shell
word — so `import_environment` now routes both through the same door. The
classification moved into `Shell::env_shell_name`, which is the testable seam:
`import_environment` itself reads the live process environment and cannot be
exercised deterministically.

Measured against bash 5.2.37 with `env 'a.b=1' '1abc=2' 'x-y=3'`: both shells
list nothing for `export -p` and `set`, and both hand all three to a child.
Covered by `an_environment_name_that_is_not_an_identifier_is_not_a_shell_variable`.
