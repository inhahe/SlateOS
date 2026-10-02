### TD-OILS-DECL-ROUTES-BEFORE-CHECKING-OPTIONS. The declaration builtins validated their flag letters only on the way to a declaration, so every listing route silently ignored an unknown one — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the `"declare" | "typeset"` and
`"local"` dispatch arms; the fix lives in the new `Shell::declare_option_check`.

**What:** bash runs **one getopt pass over the whole leading flag cluster before
it decides what the command is**, so an unknown letter is refused whatever the
rest would have done — status 2, `TAG: -X: invalid option`, and the synopsis.
osh checked the letters only on the declaration path:

```sh
declare -rq        # bash: declare: -q: invalid option + synopsis, rc 2; osh: rc 0, silent
declare -x=1       # same; osh: rc 0, silent
declare -pq v      # same; osh: reported `v: not found`
declare -Fq g / -fq g / -ftq g / -aq / -r -q / -rq -- / -iq v=1   # all rc 2
```

**Measured rule** (bash 5.2.37), pinned by the corpus case
`declaration-builtins-check-options-first.sh` and the unit test
`declaration_builtins_check_their_options_before_routing`:

- `+rq` / `+q` are reported as `+q` — the sign is echoed back.
- `-q +z` reports `-q`: the first bad letter wins, scanning left to right.
- `=` is a letter: `-x=1` → `-x`, `-=x`/`-=1` → `-=`, `-rq=1` → `-q`,
  `-a[0]` → `-[`.
- `declare -- -q` → the `--` ends the scan → rc 1
  `` declare: `-q': not a valid identifier ``, not the option error.
- `typeset` prints its own synopsis; `local` prints
  `local: usage: local [option] name[=value] ...`.
- **`local` has two refusals and the frame one comes first.** At global scope
  `local -q`, `local -pq`, `local -rq`, `local`, `local x=1`, `local -p` and
  `local -- -q` *all* give `local: can only be used in a function` rc 1 — the
  frame check precedes the option scan. Inside a function the option scan fires
  (rc 2).

**Fixed 2026-08-04.** `declare_option_check` hands the flag words **alone** to
`builtin_declare`: with no name operands that is exactly the getopt pass and
nothing more, and it keeps the single copy of the letter table. For `local` the
same call reproduces the frame refusal first for free, because
`builtin_declare_scoped` makes that check before reading a flag. Two now-dead
validators were removed rather than left as defensive cruft (the
`local_frames.is_empty()` branch in the `local` arm, and the re-validation in
the `want.is_empty()` branch).

**Standing lesson (measurement discipline):** the first probe harness for this
wrapped each command in a helper `t() { eval "$1"; }` — a *function*, which
silently put every "at global scope" measurement inside a frame and produced the
wrong rule for `local`. **Measure at the scope the construct will actually be
used in**, with bare top-level lines when scope is the variable under test.
Separately, avoid operand-less probes like `declare -I`, `declare -n`,
`declare -t`, `declare -r`, `declare +x`: each is a *listing* that dumps the
whole environment and buries the signal.
