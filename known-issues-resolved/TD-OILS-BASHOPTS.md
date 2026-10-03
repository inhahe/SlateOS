### TD-OILS-BASHOPTS. `osh` does not expose `$BASHOPTS` — RESOLVED 2026-07-20

**Resolved 2026-07-20** (superseded by later work — this entry was stale).
`osh` now seeds `$BASHOPTS` and byte-matches bash's full default set:
`echo $BASHOPTS` prints
`checkwinsize:cmdhist:complete_fullquote:extquote:force_fignore:globasciiranges:globskipdots:hostcomplete:interactive_comments:patsub_replacement:progcomp:promptvars:sourcepath`,
identical to `bash -c 'echo $BASHOPTS'`. The "honest state is unset" reasoning
below is obsolete: rather than materialize from osh's tiny live shopt inventory,
the default set is seeded directly (like `$SHELLOPTS`). Verified via probe.

<details><summary>Original entry (obsolete, for history)</summary>

**What:** bash exposes `$BASHOPTS`, a readonly colon-separated list of the
enabled `shopt` options (analogous to `$SHELLOPTS` for `set -o` options).
`osh` leaves `BASHOPTS` unset. `$SHELLOPTS` itself is now fully implemented
and byte-matches bash (see `refresh_shellopts` in `interp.rs`); `BASHOPTS`
was deliberately *not* implemented alongside it.

**Reproduce:** `bash -c 'echo "$BASHOPTS"'` prints
`checkwinsize:cmdhist:complete_fullquote:extquote:force_fignore:globasciiranges:globskipdots:hostcomplete:interactive_comments:patsub_replacement:progcomp:promptvars:sourcepath`;
`osh -c 'echo "[${BASHOPTS-UNSET}]"'` prints `[UNSET]`.

**Why deferred (not a band-aid):** bash's default `BASHOPTS` lists ~13
shopt options that are on by default. `osh` models essentially none of
those shopts (most are interactive/completion features — `progcomp`,
`hostcomplete`, `promptvars`, `checkwinsize`, `cmdhist`, `complete_fullquote`
— irrelevant to a non-interactive script shell). Materializing a `BASHOPTS`
value from `osh`'s tiny shopt inventory would produce an empty or
divergent string, which is *more* misleading to a script doing
`case $BASHOPTS in *extquote*)` than an absent variable. Shipping a fake
partial value would be exactly the kind of band-aid CLAUDE.md forbids.

**Proper fix:** implement bash's shopt inventory (at least the
default-on, behaviorally-meaningful ones osh can honestly claim —
`extquote`, `globasciiranges`, `patsub_replacement`, `sourcepath`,
`interactive_comments`, `globskipdots`, `force_fignore`), wire them into
the `shopt` builtin's option table, then add a `refresh_bashopts` helper
mirroring `refresh_shellopts` (readonly stored var, recomputed on every
`shopt -s`/`-u`). Byte-matching bash's full default set also requires
modeling the completion-related shopts, which only makes sense once osh
has a completion subsystem. Until then the honest state is "unset".
</details>
