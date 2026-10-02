### TD-OILS-DECL-DASHDASH-DOES-NOT-END-THE-P-SCAN. A `--` among the leading words did not stop the scan that looks for the listing letter, so `declare -- -p` dumped every variable — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the two inline
`args.iter().take_while(Self::is_decl_flag_word).any(|a| a.contains(&b'p'))`
scans in the `declare`/`typeset` and `local` dispatch arms.

**What:** `is_decl_flag_word` counts `--` as a flag word (correctly — it *is*
one, and the one that ends the scan), and its doc comment says the callers that
care stop at it themselves. These two callers did not, so they walked straight
past it:

```sh
declare -- -p        # bash: rc 1  declare: `-p': not a valid identifier
                     # osh: listed every variable in the shell
declare -r -- -p     # same
declare -- -p v      # same, and no listing of v
typeset -- -p        # typeset: `-p': ...
f() { local -- -p; }; f        # local: `-p': ...
f() { local -r -- -p; }; f     # local: `-p': ...
```

**Fixed 2026-08-04.** The scan is spelled once, in `Shell::declare_wants_print`,
which stops at `--` the way every other reader of the leading words already
does. Covered by the extended corpus case
`a-lone-sign-is-an-operand-not-a-flag-word.sh` and its unit test.

**Standing lesson:** the same predicate written twice inline is the shape this
family of bugs keeps taking (see TD-OILS-DECL-LONE-SIGN below, where it was
written *six* times). Give it a name the first time there is a second caller.
