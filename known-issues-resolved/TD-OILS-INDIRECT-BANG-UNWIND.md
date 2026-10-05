### TD-OILS-INDIRECT-BANG-UNWIND. `${!r}` with `r=!` under `set -u` reports once where bash reports twice — 2026-08-01 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — the indirect-expansion path
(`expand_indirect` / `resolve_indirect_target`), which reads the pointer's value
as a parameter name and raises the unbound error against the name it built.

**What.** With `set -u` and a pointer holding the one special parameter that can
be unset, bash unwinds through two layers and prints both complaints; osh prints
one, and names it differently.

```
$ bash --norc --noprofile -c 'set -u; r=!; echo "[${!r}]"'
bash: line 1: $!: unbound variable
bash: line 1: [${!r}]: bad substitution
$ osh -c 'set -u; r=!; echo "[${!r}]"'
osh: line 1: !r: unbound variable
```

Both exit 1 and neither prints the word, so only the diagnostics differ. Without
`set -u` the two agree exactly (empty, status 0), and every other pointer value
agrees under `set -u` too — it is `!` alone, because it is the only special
parameter that is ever unset (see the `$!` work in
`tests/corpus/nounset-last-bg-pid.sh`).

**Fixed.** `Shell::indirect_special_unbound` in `userspace/oils/src/interp.rs`,
called from both indirect paths (`expand_indirect` and the `IndirectOp` arm of
`expand_part`).

**Why bash does it.** Which branch of `parameter_brace_expand_word` the target
takes decides who complains. An ordinary name is looked up with
`find_variable`, and an unset one is simply *absent* — no complaint — so the
indirection reports `!r: unbound variable` against the reference as written
(`subst.c:9931`), which is what osh already did. A **special** parameter
instead goes through `param_expand`, which raises the complaint itself against
the target's own `$…` spelling and hands back a failure rather than a value;
`parameter_brace_expand` then turns that failure into `bad_substitution`
naming the whole word (`subst.c:9825`–`9829`). Hence two lines, and the second
is a DISCARD — status 1, the next line still runs — not nounset's 127 abort.

Only `$!` reaches this, and it was worth checking why: `$*` and `$@` have their
own unbound check but it is compiled out unless bash is built `STRICT_POSIX`
(`subst.c:10342`, `10465`), and a positional target is read by
`get_dollar_var_value` rather than `param_expand`, so `r=1` with no positionals
reports `!r` like any other unset name. Measured across every special parameter
(`! ? $ # - _ 0 1 @ *`).

The one thing that was **not** obvious from the entry above: the failure
happens *inside* the indirection, before the modifier is looked at. So
`${!r-D}` does not supply its default, `${!r+S}` does not stay empty, and
`${!r@Q}`/`${!r^^}`/`${!r:1:2}` all report the same two lines — bash's order,
now osh's. `${#!r}` is different again and already agreed: it is a parse-level
bad substitution, and the `$!` line never appears.

`tests/corpus/nounset-last-bg-pid.sh` covers the whole shape.
