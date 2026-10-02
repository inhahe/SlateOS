### TD-OILS-NO-STARTUP-FILES. `osh` reads no startup files at all, and so has none of the flags that suppress or redirect them — ✅ RESOLVED 2026-07-29

**Fix (2026-07-29).** osh now reads bash's startup files, and the two facts that
select them — *login* and *interactive* — are supplied by the binary rather than
guessed at by the interpreter, because both describe how the shell was
**started**:

* `Shell::run_startup_files(&StartupFiles)` (`interp.rs`) runs the table below in
  bash's order and returns `Some(status)` if one of the files exited, so the
  caller knows not to run the `-c` string/script it was about to.
  `StartupFiles { interactive, no_profile, no_rc, rc_file }` is built in
  `main.rs` from the parsed command line.
* `Shell::run_startup_file(path)` is `source` minus two things: there are no
  arguments (so the positional parameters stay the caller's) and `return`'s
  operand is **discarded** — bash reads these files with `_evalfile` *without*
  `FEVAL_BUILTIN`, so `return 5` in `~/.bash_profile` stops the file but leaves
  `$?` alone. That is what the new `SourceFrame::startup` flag marks. A frame
  *is* pushed, so diagnostics name the file, `${BASH_SOURCE[0]}` is it, and
  `return` is legal at all. Absent/unreadable is silent; a directory is reported
  with bash's `internal_error` shape (no `line N:` token — there is no command
  yet) and skipped, not fatal.
* `Shell::run_env_file` expands `$BASH_ENV` the way bash does
  (`expand_string_unsplit_to_string(…, Q_DOUBLE_QUOTES)` then
  `bash_tilde_expand`): substitutions yes, word splitting and globbing no, and a
  leading `~` afterwards. Reproduced by wrapping the value in real double quotes
  and parsing it, so `BASH_ENV='$HOME/rc'`, `BASH_ENV='/a b/rc'` and
  `BASH_ENV='~/rc'` all behave. That wrapping had a bug, fixed 2026-07-30: the
  value was interpolated raw, so a `"` in it closed the quote early and a
  **trailing** `\` escaped the closing quote and left the word unterminated —
  `BASH_ENV='/a"b/rc'` read the wrong path and `BASH_ENV='/rc\'` silently read
  none. Exactly those two bytes are now escaped, and no others: a `\` anywhere
  but the end already means inside real double quotes what it means in bash's
  `Q_DOUBLE_QUOTES` mode, so escaping it there would corrupt `BASH_ENV='\$x'`.
* `~/.bash_logout` is *not* part of that table. bash reads it from inside the
  `exit`/`logout` builtin (`exit_or_logout` → `bash_logout`), which is what
  confines it to a **login** shell leaving through that builtin — never on
  falling off the end of a script, never for a subshell's exit. So the builtin
  *arms* a new `Shell::logout_pending: Option<i32>` and `run_logout_file()`
  (called by the binary before the `EXIT` trap) reads it. Being inside the
  builtin also explains its two odd details, both reproduced: it goes **before**
  the EXIT trap, and `$?` inside it is the status from *before* the `exit`,
  because the operand has not been recorded yet.
* The four flags now exist for real — `--noprofile`, `--norc`,
  `--rcfile FILE`/`--init-file FILE` — plus `--verbose` (= `set -v`) and
  `--noediting` (accepted, no line editor to disable).

Only the *vanilla* bash set is implemented: `/etc/bash.bashrc` is a distro
`-DSYS_BASHRC` patch, not upstream behaviour, so osh does not read it.

**Not fixed here** (each small, none blocking): `$ENV`, which bash reads
*instead* of `~/.bashrc` for an interactive shell in POSIX mode or named `sh` —
osh has neither state, so nothing could select it; and bash's remaining long
options (`--posix`, `--restricted`, `--debugger`, `--wordexp`), deliberately
still errors for the same reason the startup flags used to be: accepting them
would make osh *claim* behaviour it lacks.

**Where the fix landed:** `interp.rs` (`run_startup_files`, `run_logout_file`,
`run_startup_file`, `run_env_file`, `StartupFiles`, `SourceFrame::startup`,
`logout_pending`, the armed `exit`/`logout` arm and the startup-aware `return`
arm), `main.rs` (a rewritten two-pass command line — see
TD-OILS-INVOKE-OPTION-PASS for the four parser bugs that rewrite exposed), and
`lib.rs` (re-exports `StartupFiles`).

**Tests.** `tests/corpus/startup-files.sh` (new, byte-identical to bash 5.2)
covers the whole table plus the long-option pass's diagnostics; 8 new tests in
`tests/cli_options.rs` drive the real binary with an isolated `$HOME`; 6 new
unit tests in `interp.rs` exercise the library API directly. The corpus harness
still forces `BASH_ENV=""`/`ENV=""` for both shells, and a case that re-invokes
`"$BASH" -l …` must still marker-filter the child's output, because a real
`/etc/profile` exists on the reference system and not under osh.

---

**Original report.**

**Where:** `userspace/oils/src/main.rs` — the option loop and the
`import_environment()` / mode-dispatch sequence that follows it. Nothing in
`interp.rs` is involved yet; the state a startup file would set already exists,
only the reading of one does not.

**What.** bash picks its startup files from *how it was started*, and osh reads
none of them:

| How started | bash reads | osh reads |
|---|---|---|
| login shell | `/etc/profile`, then the first of `~/.bash_profile`, `~/.bash_login`, `~/.profile` that exists; `~/.bash_logout` on exit | nothing |
| interactive non-login | `/etc/bash.bashrc` (Debian-patched builds), `~/.bashrc` | nothing |
| non-interactive with `$BASH_ENV` set | the expanded value of `$BASH_ENV` | nothing |
| `sh`-named non-interactive with `$ENV` set | the expanded value of `$ENV` | nothing |

Consequently osh also rejects the four invocation flags whose entire job is to
change that behaviour — `--noprofile`, `--norc`, `--rcfile FILE`,
`--init-file FILE` (the last two synonyms) — with its
"invalid option" diagnostic. `scripts/osh-bash-diff.py` papers over the
difference by forcing `BASH_ENV=""` and `ENV=""` for both shells, and by passing
`--norc --noprofile` to the *reference bash only*; a corpus case that re-invokes
`"$BASH" -l -c …` must therefore marker-filter the child's output, because the
reference side may chatter from a real `/etc/profile` while osh cannot.

**Why it is not just a missing flag.** Adding `--norc`/`--noprofile` as accepted
no-ops would make osh *claim* a behaviour it does not have: a script probing
`--norc` would conclude rc files exist and are suppressed, when in fact none are
ever read. The flags are meaningless until the reading exists, so they are
deliberately still errors.

**Proper fix.** Implement the reading, then the flags fall out of it:

1. Add a `Shell::run_startup_file(path, missing_ok)` that is `source` with the
   not-found case silenced — same `source_stack` push, same `$0`/`$LINENO`
   handling, same `Flow::Abort` propagation, so a syntax error in `~/.bashrc`
   behaves as bash's does.
2. In `main.rs`, after `import_environment()` and after the option loop has
   settled `login_shell`/interactivity but *before* the `-c` string or script
   file is read, run the table above. Word-expand `$BASH_ENV`/`$ENV` first
   (bash expands but does not path-search them).
3. Register `~/.bash_logout` as a login-shell exit action next to the existing
   `EXIT` trap firing, so it runs last and only for a login shell.
4. Accept `--noprofile`, `--norc`, `--rcfile FILE` and `--init-file FILE` in the
   long-option arm beside `--login`, each simply suppressing or replacing one row
   of the table.
5. The paths are host-dependent, so put the two absolute ones
   (`/etc/profile`, `/etc/bash.bashrc`) behind a single constant that the
   slateos target and the Windows dev host can spell differently — the corpus
   runs on the host, where `/etc/profile` is MSYS's.

**Impact.** None on script semantics — every corpus case and every unit test
runs a shell that would read nothing anyway. It bites the moment osh is a *user's*
login shell on SlateOS: `PATH`, `PS1`, aliases and shell functions from
`/etc/profile` and `~/.bashrc` would all be silently ignored. It also caps how
much of the invocation surface the differential corpus can pin, since the flags
that control startup are the ones osh rejects.
