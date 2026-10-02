### TD-OILS-INTERACTIVE-SHELL-VS-INTERACTIVE. `-i -c` does not report `i` (or `H`) in `$-`, because one flag models two of bash's — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/interp.rs` — `Shell::is_interactive()` (≈ line
3180), which is `!self.command_mode && !self.script_mode && self.repl_interactive`,
and every caller of it.

**What.** bash has *two* globals here, and they are not the same thing:

* `interactive_shell` — "was this shell **started** interactively", i.e. `-i` or
  a tty with no script/`-c`. Set once at startup and never changed. It is what
  puts `i` in `$-`, what turns `histexpand` (`H`) on by default, and what
  `~/.bashrc` reading keys off.
* `interactive` — "is the shell **currently** reading from a prompt". False
  inside a `-c` string, a sourced file, a function, a subshell.

osh has one flag standing in for both, so an `-i -c` shell reports neither:

```
$ bash --noprofile -i -c 'echo "[$-]"'    →  [hiBHc]
$ osh  --noprofile -i -c 'echo "[$-]"'    →  [hBc]
```

**Impact.** Small but real. `$-` is the documented way a script asks "am I
interactive", and the idiom `case $- in *i*) …` is common in `~/.bashrc` files —
under osh such a file would take the non-interactive branch even in an
interactive shell. `H` is wrong for the same reason. Note this is *not*
TD-OILS-INTERACTIVE-DETECT (fixed 2026-07-20), which was about *detecting*
interactivity from the tty; this is about osh collapsing two distinct pieces of
state into one field.

**Fix (2026-07-29).** `Shell::interactive_shell` is now bash's startup-time
global — set once by `main.rs` from the same computation that feeds
`StartupFiles::interactive`, never changed afterwards — and `is_interactive()`
returns it directly. bash's *dynamic* `interactive` turned out not to need a
field of its own: the only places that want it are the ones that already test
the condition explicitly (`eval_depth > 0` and a backtick body both force the
`line N:` token, because bash clears `interactive` for the duration of a command
substitution), and the prompt loop, which is only ever entered by a REPL. So the
old mode-derived `repl_interactive` was folded away rather than kept alongside —
two fields where one is enough would have been the same trap in a new shape.
`$-`'s `i`, the `expand_aliases`/`histexpand`/`history` defaults, prompt
printing, and the `line N:` token now all key off `interactive_shell`. A
separate `reads_stdin` field carries bash's third, independent flag
(`read_from_stdin` — `-s`, or no operand left to run as a script), which is what
lets `-cs` report both `c` and `s`. `set_interactive_shell()`
refreshes **both** option snapshots, because `$BASHOPTS` carries
`expand_aliases` and `$SHELLOPTS` carries `histexpand` and `history` — missing
the second one left `$SHELLOPTS` stale, which is how the bug was found.

The whole `$-` matrix was measured against bash and now matches byte-for-byte:
`-c`→`hBc`, `-i -c`→`hiBHc`, `-cs`→`hBcs`, `-i -cs`→`hiBHcs`, `-H -c`→`hBHc`,
`-i +H -c`→`hiBc`, a script→`hB`, `-i script`→`hiBH`, and `-s`/`-`/bare
stdin→`hBs`. There is no `m` even under `-i`, because job control cannot be
enabled on a pipe.

Two knock-on notes:

* `Shell::new()`'s default is *interactive*, which is the pre-existing contract
  the test harness documents (`run()` is a prompt, `run_script()` is a script).
  `run_script()` now calls `set_interactive_shell(false)` and its doc comment
  lists all five ways the two now differ — the difference set grew, so it had to
  become visible. The 11 test-module `set_command_mode()`/`set_script_mode()` call
  sites say `set_interactive_shell(false)` explicitly rather than having
  `set_command_mode()` imply it, because that implication is the coupling that
  caused this bug in the first place.
* `job_control_enabled` was deliberately left alone. bash on this machine (MSYS)
  does not enable job control even under `-i`, and slateos has no job control at
  all, so there is nothing to route it to yet.

**Remaining deliberate divergences in this area** (all verified, none accidental):

* `$SHELLOPTS` omits `emacs`. bash lists it because readline is compiled in;
  osh has no line editor, so claiming `emacs` would make `[[ -o emacs ]]` lie.
* Under `-i`, bash shortens `$0` in diagnostics to `bash` where osh prints the
  full program path. Pre-existing and tracked separately.

**Deferred alongside it:** osh does not validate a `-O` name against
`SHOPT_TABLE` any earlier than `apply_shopt_option` does, so a name that is a
*valid* shopt but unimplemented would be accepted where bash might not. No known
divergence; noted only so the next reader does not assume it was checked.
