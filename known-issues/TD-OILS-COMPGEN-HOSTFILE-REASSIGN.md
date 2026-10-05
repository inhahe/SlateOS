### TD-OILS-COMPGEN-HOSTFILE-REASSIGN. A *redundant* `HOSTFILE=` assignment offers the file's hostnames a second time in bash; osh treats it as the no-op it reads as — OPEN 2026-07-31 (deliberate)

**Where:** `userspace/oils/src/interp.rs` — `Shell::compgen_hostnames` and the
`hostname_list`/`hostname_source` pair it caches into.

**What.** bash keeps the hostname list between completions, and re-reads
`$HOSTFILE` whenever the variable is **assigned** rather than whenever its value
changes. Since `snarf_hosts_from_file` appends, assigning the same path twice
offers everything in it twice:

```sh
HOSTFILE=h.txt; compgen -A hostname   # both: one
HOSTFILE=h.txt; compgen -A hostname   # bash: one one   osh: one
```

Everything else about the list matches byte-for-byte and is covered by
`tests/corpus/compgen-hostname.sh`: what a line contributes (first field is the
address, `#` starts a comment wherever it appears, no dedup), that naming a
*different* file **adds** to the list, that *unsetting* `HOSTFILE` throws the
list away first, and that an unreadable file contributes nothing without being
an error.

**Proper fix.** None wanted. osh compares the value it last read the list from,
which is what the manual describes ("the next time hostname completion is
attempted **after the value is changed**"); bash's extra copy falls out of
hanging the invalidation off the assignment hook instead. Reproducing it would
mean threading a side effect through every path that can write a variable —
`HOSTFILE=x cmd`, `read HOSTFILE`, `declare`, `printf -v`, a `local` going out
of scope — purely to emit duplicate output. The *unset* half genuinely is
behaviour, not an artefact, and osh does hook it (`Shell::unbind_var`).

**Impact.** Duplicate candidates after a redundant assignment, in a shell with
no interactive completion to show them. Kept out of the corpus on purpose.
