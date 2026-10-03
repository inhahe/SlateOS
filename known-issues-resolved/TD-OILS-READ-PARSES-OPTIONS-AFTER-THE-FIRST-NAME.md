### TD-OILS-READ-PARSES-OPTIONS-AFTER-THE-FIRST-NAME. `read` keeps scanning for options past its first operand, where bash stops — 2026-08-01 — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — the `read` builtin's argument
loop, which walks every argument looking for a leading `-` instead of
stopping at the first word that is not one.

**Reproduce** (found while probing chained dups, `target/dvscratch/t3/pv.sh`):

```sh
exec 3<in; read -r a -u 3; echo "a=[$a]"
```

bash stops option parsing at `a`, so `-u` and `3` are *names*, and `-u` is
not a valid one:

```
bash: read: `-u': not a valid identifier
```

osh reads a line from fd 3 into `a` instead. The same holds for any option
letter written after a name.

**Fix.** The option loop's non-option arm pushed one name and went on
scanning; it now takes the rest of the argument list as names and breaks.
Everything after the first operand is therefore a name, and a bad one is
refused by the identifier check that was already there — the *first* name
before the record is read, a later one only as the assignment reaches it,
which is why `read a -r` still leaves `a` holding the first field.

A `--` is consumed as a terminator only when the scan is still running; the
arm above the change handles that, and after a name it falls through to the
names like any other word. A lone `-` was never taken as an option (the arm
tests `len > 1`), so it already ended the scan by being a name.

Measured the same question of `mapfile`, `unset`, `export`, `printf`,
`pwd`, `type` and `local` (`target/dvscratch/t3/pt.sh`): all seven already
match bash, so only `read` had the shape.

Covered by `tests/corpus/read-stops-scanning-options-at-the-first-name.sh`
plus `read_stops_scanning_options_at_the_first_name`.
