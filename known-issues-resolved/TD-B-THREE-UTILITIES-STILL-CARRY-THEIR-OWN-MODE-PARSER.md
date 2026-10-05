## TD-B-THREE-UTILITIES-STILL-CARRY-THEIR-OWN-MODE-PARSER (lane B, 2026-08-22) — RESOLVED 2026-08-22

**In short:** "Mode" here means the thing you type after `chmod` — `755`, or
`u+w`, or `a+rX,go-w`. Several places in the tree each wrote their own parser
for it and disagreed, so a script that set a permission with one tool and read
it back with another could get two answers. There is now one parser,
`userspace/modechange`, and every utility that speaks the coreutils mode
grammar uses it. The one parser that remains separate — the shell's `umask`
builtin — is deliberate, and is explained at the bottom of this entry: it is
not the same grammar.

**Where it lives:**

| Site | State |
|---|---|
| `userspace/coreutils/src/bin/chmod.rs` | **Done** — rewritten on `modechange`, 24,480 GNU rows agree. |
| `userspace/coreutils/src/bin/stat.rs` | **Done** — `format_mode` now calls `modechange::permission_string`. |
| `userspace/coreutils/src/bin/mkdir.rs` | **Done** — `-m` implemented on `modechange` (base `0777`, real umask, `dir=true`). |
| `userspace/coreutils/src/bin/mkfifo.rs` | **Done** — same, base `0666`, `dir=false`. |
| `userspace/install/src/main.rs` | **Done** — base `0`, umask `0`; see below. |
| `userspace/chown/src/main.rs` | **Done** — this crate's `chmod` persona now resolves even an octal mode against the current mode, so `-R 755` no longer strips setgid off a directory. |
| `userspace/oils/src/interp.rs` | **Stays separate, on purpose** — a different grammar, see below. |

**The five callers disagree on all three arguments to `adjust`, so there is no
shared "apply the mode" helper above the parser** — only a shared parser. This
was the surprise of the conversion and is worth keeping written down:

| | `mkdir -m` | `mkfifo -m` | `install -m` | `chmod` | `chmod --reference` |
|---|---|---|---|---|---|
| Base mode | `0777` | `0666` | **`0`** | the file's current mode | the file's current mode |
| `dir` | `true` | `false` | `true` for `-d`, else `false` | per-file `is_dir()` | per-file `is_dir()` |
| Umask | the real umask | the real umask | **`0`** | the real umask | **`0`** |

`install` is the odd one twice over, and both were measured against GNU 9.4
across umasks `000 022 077 002 027`: every `install -m` answer is identical
under all five, and the base of `0` means a `-m` spec *describes the finished
mode* rather than editing a default — `install -m u+w` is `0200`, not `0755`
with a write bit added, which is what the old hand-written parser produced.

**Why the shell's `umask` builtin is not converted, and must not be.** It is
bash's grammar, not GNU coreutils'. Measured against bash 5.2.21:

| | coreutils / `modechange` | bash `umask` |
|---|---|---|
| `X`, `s`, `t` | accepted | rejected — `` `X': invalid symbolic mode character `` |
| copy-source triads (`u=g`) | accepted | rejected — `` `g': invalid symbolic mode character `` |
| several operators per clause (`u+r-w`) | accepted | rejected — `` `-': invalid symbolic mode character `` |
| octal ceiling | `07777` kept whole | parsed to `07777`, then masked to `0777`; `10000` is "octal number out of range" |
| a clause with no `who` | filtered by the umask | no umask is involved — the value *is* the umask |
| operates on | the file's mode bits | the **complement**, i.e. the permission set, re-complemented at the end |
| diagnostic | `chmod: invalid mode: ‘zzz’` | `` umask: `z': invalid symbolic mode operator `` |

Putting `umask` on `modechange` would *widen* the language the shell accepts —
`umask u+X`, `umask u=g` and `umask 10000` would all start succeeding — and
would lose the per-character diagnostics, which name the exact offending
character and distinguish a bad operator from a bad permission letter. That is
a regression wearing the clothes of a cleanup. The shell keeps
`parse_symbolic_umask`, and its module doc now says why.

**What the previous entry got wrong, and it matters for whoever picks this up:**
that entry called the prerequisite "lifting `chmod`'s parser into a shared
`coreutils::mode` module". It became a **separate crate**, `userspace/modechange`,
not a `coreutils` module, because three of the six remaining callers —
`install`, `chown` and the shell — are their own crates and must not depend on
`coreutils`; the shell especially, since `coreutils` is the set of programs the
shell *runs*. See design-decisions §364. A `coreutils::mode` module would have
reconciled `chmod` with `mkdir` and left the other four disagreeing, which is
worse than it sounds: three of four agreeing makes the fourth look like a local
bug rather than a missing abstraction.

**Measured GNU behaviour for the two that are unimplemented**, so nobody has to
re-measure (GNU coreutils 9.4, under WSL):

- Both `mkdir -m` and `mkfifo -m` compile the mode against a **starting mode of
  `0777`** — not `0755`, and not `0666` for the FIFO — then set the umask to
  zero for the actual creation. So `mkdir -m 700 d` is `0700` under **every**
  umask, which is the whole point of the option.
- The umask still reaches clauses with no `who`, because that is `modechange`'s
  rule rather than the caller's: `mkdir -m 'a=,+w'` is `0222` under umask 000,
  `0200` under 022 **and** under 077, `0220` under 002.
- `dir` differs, and `X` is what shows it: `mkdir -m 'a=,+X'` is `0111`,
  `mkfifo -m 'a=,+X'` is `0000`. Note this also pins that `X` consults the mode
  *as accumulated so far* — after `a=` there is no execute bit left, so on the
  FIFO it does not fire.
- With `-p`, only the **last** component gets the requested mode; the
  intermediates get the ordinary `0777 & ~umask`. Measured: `umask 022; mkdir -p
  -m 700 a/b/c` gives `a` 755, `a/b` 755, `a/b/c` 700.
- `-m` on a directory that already exists does nothing, even with `-p`:
  `mkdir a; chmod 700 a; mkdir -p -m 755 a` leaves it `0700`.

**The three invalid-mode wordings are all different**, which is exactly the kind
of thing the "measure the specific message, not the utility next door" rule
above exists for:

```text
chmod:  chmod: invalid mode: ‘zzz’      (colon, curly)
mkdir:  mkdir: invalid mode ‘zzz’       (no colon, curly)
install: install: invalid mode ‘zzz’    (no colon, curly)
mkfifo: mkfifo: invalid mode            (no colon, and the string is not quoted at all)
```

`mkfifo`'s is not a typo in this entry — GNU really does drop the operand from
that one message.
