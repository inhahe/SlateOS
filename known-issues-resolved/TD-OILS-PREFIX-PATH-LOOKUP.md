### TD-OILS-PREFIX-PATH-LOOKUP. A `PATH=… cmd` prefix assignment does not reach osh's own `$PATH` search — 2026-08-02 — ✅ **RESOLVED 2026-08-02**

**Resolution.** The temporary environment now reaches the search. `Shell::temp_path`
picks the last `PATH=` out of a command's prefix assignments, and
`Shell::find_in_path` takes it as an override that outranks `self.param_value("PATH")`
— which is bash's own rule, since `get_string_value` consults `temporary_env` first
and `find_user_command` is no exception. Two wrappers pick the right table policy
for the caller:

* `resolve_external_with` — for a command run *in this shell*. Without a prefix
  `$PATH` it is the ordinary `resolve_external` (hash consulted, hit counted, miss
  remembered). With one, the `hash` table is out of the picture entirely: neither
  consulted nor added to, so `hash -p da/w.sh w.sh; PATH=db w.sh` runs *db's* copy.
* `resolve_external_forked` — for a pipeline stage or a `&` job. bash forks before
  the lookup, so the table is *read* (the fork inherited it) but never written back.

Binding `PATH` empties the table (bash's `stupidly_hack_special_variables` →
`sv_path` → `phash_flush`), and a prefix binds it — so the flush is done at the
simple-command site rather than inside `prefix_assignments`, because a forked
command's prefix must not reach *this* shell's table. `note_var_change` now also
rebuilds `BASH_CMDS`, which is the same table seen as an associative array and
would otherwise show entries the table no longer holds.

Four adjacent divergences found alongside it and fixed in the same pass:

* **`$_` counted a second hit.** `apply_child_env` re-ran the search to name the
  child's program. The already-resolved path now travels with the word as a
  `ChildProgram`, so one command is counted once.
* **Reported paths carried the host's separator.** `search_dirs` spells each
  directory the way the shell spells paths (`/`), because a hit is not only
  spawned but printed by `hash`, `type` and `$_`. The host's spelling is restored
  at the single `PCommand::new`, by `host_program` — and only there, because a
  program that re-parses its own command line reads a leading `/` as an option
  (`cmd.exe` handed `C:/WINDOWS/system32/cmd.exe` fails with
  `Error occurred while processing: .exe`).
* **An empty `$PATH` element** now means the shell's own directory, which is
  POSIX's reading and bash's — see the divergence noted below on how it is spelled.
* **A bare word the search could not place was handed to the OS anyway**, which
  ran a search of its own — and on Windows `CreateProcess` searches the *parent
  process's* `PATH`, not the shell's, so `PATH=/nowhere sed …` still found a `sed`.
  bash never asks the OS to find anything: it resolves and then `execve`s a full
  path. `spawn_resolved` now refuses to start an unresolved bare word, which makes
  it `command not found` (127). A word that spelled a *path* is still handed over
  unresolved on purpose — only the OS can say why that file will not run.
  `find_all_in_path` (`type -a`) was also moved onto the same `search_dirs` and the
  same executability test, so it can no longer disagree with `find_in_path`.

Covered by `tests/corpus/path-prefix-assignment-search.sh` (byte-identical to bash
5.2.37 across all eleven sections) plus four `interp.rs` unit tests:
`the_search_path_is_the_shell_s_own_and_a_prefix_outranks_it`,
`a_program_is_spawned_by_the_host_s_spelling_of_its_path`,
`a_forked_stage_reads_the_hash_table_but_cannot_change_it` and
`an_unresolved_bare_word_is_not_handed_to_the_os_to_find`. Full suite green
(1153 + 4 + 39 + 7 + doctests), clippy clean, corpus sweep 263 matched / 0 failed.

**Known residual divergence, deliberate.** bash spells a hit found through an
empty (or relative) `$PATH` element *relatively* — `./w.sh` — and refuses to hash
it. osh keeps it absolute, because Windows `CreateProcess` resolves a relative
application name against the calling *process's* directory rather than the
`current_dir` given to the spawn, so a relative program path would launch the
wrong file (or nothing) whenever the shell's `$PWD` is not the process's. The
divergence is visible only in what `hash`/`type`/`$_` print for such a hit; the
file run is the same one. Revisit if osh ever resolves the program image itself
rather than through `std::process::Command`.

**Where:** `userspace/oils/src/interp.rs` — `Shell::prefix_assignments` returns
the temporary environment as a plain `Vec`, which only ever reaches the child
through `apply_child_env`. `Shell::find_in_path` reads `self.param_value("PATH")`
and so never sees it.

**What:** bash keeps the prefix assignments in `temporary_env`, which
`get_string_value` consults *first* — so `find_user_command` searches the new
`$PATH`. osh searches the old one, fails to resolve, and hands the bare word to
the OS, which then searches the *child's* `$PATH` and gets a different answer —
and, for a script, one osh never got the chance to classify.

```
$ PATH=/dir cmd      # bash: runs /dir/cmd
                     # osh:  the shell's search misses; the OS finds it via the
                     #       child environment, so the shell never sees the file
```

**Proper fix.** Thread the temporary environment into the lookup: give the
external-command paths a `$PATH` that prefers a `PATH=` among the prefix
assignments. A hit found that way must **not** be entered into the `hash`
table — bash does not hash it (`PATH=dir cmd; hash` reports an empty table,
where a real `PATH=dir; cmd` does hash the hit).

**Impact.** `PATH=… cmd` finds the wrong command, or the right one by accident.
Found while writing `tests/corpus/exec-shebangless-script.sh`, which uses a real
assignment inside a subshell to work around it.
