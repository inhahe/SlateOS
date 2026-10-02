### TD-OILS-AN-EMPTY-MAPFILE-CALLBACK-IS-TREATED-AS-NO-CALLBACK. `mapfile -C ""` runs nothing in osh; bash runs the index as a command — 2026-08-08 — ✅ **RESOLVED 2026-08-08**

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_mapfile`,
`let fire_callback = callback.as_ref().is_some_and(|c| !c.is_empty()) && …`.

**What.** bash's test is a null-pointer test on a `char *`, not an emptiness
test — `if (callback && line_count && (line_count % callback_quantum) == 0)`
(`builtins/mapfile.def:206`) — and the callback string is pasted into a command
line unconditionally: `snprintf (execstr, execlen, "%s %d %s", callback,
curindex, qline)` (`:131`). With an empty callback that command line is
` 0 'a'`, whose command word is the *index*.

```sh
mapfile -t -C "" -c 1 q <<< $'a\nb'
# bash: 0: command not found
#       1: command not found
# osh : (nothing)
# both: rc=0, declare -a q=([0]="a" [1]="b")
```

`-c 0` is refused by both shells before this point (`invalid callback quantum`),
so the `quantum != 0` half of osh's guard is unreachable and only the emptiness
half is wrong.

**How it was found.** Probing which kinds of nested command blank bash's
`this_command_name` for `TD-OILS-BUILTIN-TAG-SURVIVES-A-NESTED-COMMAND`: the
`-C ""` row was the one place the two shells still differed after that fix, and
the difference was that osh had not run a command at all.

**Fixed in `HEAD`.** The guard is now `callback.is_some() && quantum != 0`.
osh already built the callback command line the way `run_callback` does —
`bfmt![cb, b" ", idx, b" ", sh_single_quote(line)]`, i.e. `"%s %d %s"` — so an
empty callback needed nothing else: it yields a leading space and the index
becomes the command word. (The quantum half of the guard is defensive only:
`-c 0` is refused up front by both shells, and `is_multiple_of(0)` would divide
by zero.)

Measured against bash 5.2.37 — every row matched after the change, including
the ones that pin *which* index is used and *when* it fires:

| | bash and osh |
|---|---|
| `-C '' -c 1 a <<< $'x\ny'` | `0: command not found`, `1: …`; `a=(x y)`, rc 0 |
| `-C '' -O 5 -c 1` | `5: command not found` — the *stored* index |
| `-C '' -c 2` over five lines | fires at `1` and `3` |
| `-C ' '` | same as `-C ''` |
| `-C '' < /dev/null` | nothing fires |
| `-C '' ` without `-t` | array keeps its delimiters; callback unaffected |

Covered by the lib test `mapfile_an_empty_callback_makes_the_index_the_command`
and the corpus case
`an-empty-mapfile-callback-makes-the-index-the-command.sh`.
