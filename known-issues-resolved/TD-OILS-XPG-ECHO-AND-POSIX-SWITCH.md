### TD-OILS-XPG-ECHO-AND-POSIX-SWITCH. `shopt -s xpg_echo` did nothing and `set -o posix` was completely inert — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `builtin_echo`, `apply_shopt_option`,
`shell_option_enabled`, `refresh_shellopts`, `note_var_change`, `put_var`,
`unbind_var`, `restore_var`, `pop_temp_shadow`, `run_builtin`/`run_builtin_body`
(now taking a `BuiltinCall`).

**The two gaps.** `xpg_echo` was in the `shopt` table but no code read it, so it
was a switch wired to nothing. `posix` was worse: it was in
`STANDARD_SET_O_OPTIONS` and so *listed* by `set -o`, but it had no state at all
— it fell into the "accept as a no-op" arm, always reported `off`, never
appeared in `$SHELLOPTS`, and `[[ -o posix ]]` was permanently false even
immediately after `set -o posix`. Five divergences, all measured against bash
5.2.37:

1. **`xpg_echo` did not move `echo`'s default.** With it set, escapes are
   interpreted without `-e` (same dialect: `\0101`→`A`, a bare `\101` raw,
   `\x41`, `\u0041`), `-E` turns them back off, and `-n` and `\c` are unchanged.
2. **`set -o posix` was a no-op.** bash keeps posix mode *in*
   `$POSIXLY_CORRECT`: the option and the variable are one switch. Any value —
   including the empty string, including via a `local` or an assignment prefix —
   turns it on, and unsetting turns it off. `set -o posix` writes `y` only when
   the name has no value; `set +o posix` unsets it, reaching **past `readonly`**
   (it is the shell unbinding the variable, not the `unset` builtin refusing).
   osh now derives the option from the variable rather than mirroring it into a
   field, which is what makes `f() { local POSIXLY_CORRECT=1; }` posix mode for
   the call and no longer.
3. **`$SHELLOPTS` did not follow a bare assignment.** `POSIXLY_CORRECT=1` puts
   `posix` in `$SHELLOPTS` just as `set -o posix` does. That needed the
   per-name hook (`note_var_change`) to fire on *every* path a name can gain or
   lose a value, so the hook was moved to run **after** the store/removal (it
   derives state from the new value) and was added to `restore_var`, the
   scope-pop path — which also fixes a latent `PATH` bug: a
   `f() { local PATH=x; }` going out of scope did not invalidate the command
   hash.
4. **posix + `xpg_echo` did not suppress the option scan.** The two together
   make `echo` read *no* options at all: `echo -n x` writes `-n x` and a
   newline, with the escapes still interpreted. Posix mode alone changes
   nothing.
5. **An assignment prefix on a special builtin did not persist.** POSIX makes it
   an ordinary assignment, and bash obeys that in posix mode:
   `set -o posix; A=1 eval :` leaves `A=1`. It lands through the *ordinary*
   assignment path, so `declare -i n=9; n=3+4 eval :` leaves
   `declare -ix n="7"` — the `-i` of the name the prefix displaced applies to
   the prefix's value, and the prefix's export marking stays. `command` strips
   the special class off (`A=1 command eval :` keeps nothing) where `builtin`
   does not; a prefix on a *function* never persists in bash 5.2. This is what
   makes the switch self-referential: `POSIXLY_CORRECT=1 eval :` is the prefix
   that turns the rule on, so it keeps itself.

**Coverage added:**
`tests/corpus/xpg-echo-moves-echos-default-and-posix-takes-its-options-away.sh`,
`tests/corpus/posix-mode-is-the-posixly-correct-variable.sh`, and four lib tests
named for the rules they pin.
