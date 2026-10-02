### TD-OILS-DYNVAR-UNSET. `unset SECONDS` did not take the value function with it, and `PPID` was not readonly — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::dyn_unset`,
`dynamic_special_entry` / `dynamic_special` / `dynamic_special_live_names` /
`dynamic_special_listed`, `dynamic_special_value` (extracted out of
`param_value`), the `is_var` test in `builtin_unset`, the `BASH_ARGV0` /
`RANDOM` / `SECONDS` / `SRANDOM` interceptions in `apply_assignment_inner`,
`var_names_with_prefix`, `declare_p_names`,
`format_dynamic_special_declare`, and the `PPID` seed in the startup
variable block.

bash implements ten of its special variables as names carrying a **value
function**: `SECONDS`, `RANDOM`, `SRANDOM`, `LINENO`, `BASHPID`,
`BASH_SUBSHELL`, `EPOCHSECONDS`, `EPOCHREALTIME`, `HISTCMD` and
`BASH_ARGV0` hold no stored value, and each read calls a function that
computes one. `unset` removes the whole variable, that function included,
and bash never puts it back — the name is left unset and entirely
**ordinary**. osh answered all ten from hard-coded arms in `param_value`,
which no `unset` could reach, so every one of them stayed dynamic forever:

```sh
unset SECONDS; echo "[${SECONDS-UNSET}]"      # bash [UNSET]   osh (before) [0]
unset RANDOM; RANDOM=5; echo "$RANDOM $RANDOM"
                                              # bash "5 5"     osh (before) two draws
unset SECONDS; SECONDS=5; declare -p SECONDS
                             # bash declare -- SECONDS="5"   osh declare -i SECONDS="0"
unset LINENO; echo "[$LINENO]"                # bash []       osh (before) [8]
unset EPOCHSECONDS; echo "${!EPOCH@}"
                             # bash EPOCHREALTIME            osh both names
```

Three separate things were wrong. `builtin_unset` never even called
`unbind_var` for these names — its `is_var` test only consulted the
variable tables, so `unset SECONDS` fell through to the *function*
namespace and silently did nothing. `param_value`'s arms took precedence
over every table. And the assignment interceptions (`RANDOM=n` reseeds,
`SECONDS=n` rebases, `SRANDOM=n` is swallowed, `BASH_ARGV0=x` sets `$0`)
belong to the dynamic binding too, so they had to stop firing once it was
gone.

`PPID` is the one member of the family this must *not* happen to: bash
makes it readonly, so every write path refuses it (`PPID: readonly
variable`) and `unset PPID` fails with `cannot unset: readonly variable`.
osh had the `r` in its `DYNAMIC_SPECIALS` row — enough for `declare -p
PPID` and `readonly -p` — but no entry in `Shell::readonly`, so `PPID=5`,
`(( PPID = 5 ))`, `declare PPID=5` and `export PPID=5` were all silently
accepted. (`printf -v`, `read` and `for` appeared to refuse it only
because the probe had run `readonly PPID=5` first.)

**Fixed 2026-07-31.** `Shell::dyn_unset` is a per-shell set of the dynamic
names `unset` has dropped, cloned into subshells so a subshell's `unset`
does not reach the parent. `unbind_var` populates it for any
`DYNAMIC_SPECIALS` row; `dynamic_special` (now a `&self` method over the
raw `dynamic_special_entry` lookup) and `dynamic_special_live_names`
answer `None`/skip for a dropped name, which is what removes it from
`declare -p`, from the flag-filtered listings and from `${!prefix@}`. The
value arms moved out of `param_value` into `dynamic_special_value`, which
consults the set and is called from `param_value`'s fall-through so a
dropped name reaches the ordinary tables. `builtin_unset`'s `is_var` test
gained `dynamic_special(a).is_some()`. And `PPID` now joins `UID`/`EUID`
in `Shell::readonly` at startup, which makes all eight write paths refuse
it and `unset PPID` fail, exactly as measured. Corpus case
`dynamic-var-unset.sh`; unit test
`unset_drops_a_dynamic_variables_value_function`.
