## TD-B-GETOPT-HAS-NO-DRIVER-FOR-OPTIONS-THAT-TAKE-VALUES (lane B, 2026-08-22) — RESOLVED 2026-08-22

**Resolved.** The trigger fired: `realpath` is the second bin needing a
value-taking option (`--relative-to=DIR`, `--relative-base=DIR`), so the walk was
lifted rather than copied. `coreutils::getopt` now has `Program::parse` /
`parse_aliased`, the `Opt` enum and the `Parser` iterator; `touch` was converted
onto it and its private `Cursor`, `parse_long`, `parse_cluster`,
`short_takes_argument` and `apply_short*` are gone. All 45 of `touch`'s tests
passed **unchanged** across the swap, which is the evidence that the lift is
behaviour-preserving.

Two deliberate deviations from the plan below, both of which the plan got wrong:

- **The signature is lazy, not eager.** `-> Result<Vec<Opt>, Error>` would have
  broken `--help`: measured, `readlink --help --bogus` prints the help and exits
  0, while `readlink --bogus --help` is an error. A parser that validated all of
  argv before returning cannot produce the first. `parse` returns an
  `Iterator<Item = Result<Opt, Error>>` instead, so a caller acts on each item
  before asking for the next — which is what `getopt_long` itself does.
- **`Takes::Optional` now has a design and a test**, though still no bin using
  it: an optional value is *never* the next word, only the glued one. That was
  the case the entry said would shape the design, and it did — it is the whole
  difference between `--check x` (operand) and `--key x` (value).

The tests named below as the specification moved into `getopt.rs` alongside the
code, keeping `touch`'s table as their subject. `touch`'s own copies stayed, now
testing one layer up: that each `Opt` is wired to the right field.

The original entry follows.

---

**TD-B-GETOPT-HAS-NO-DRIVER-FOR-OPTIONS-THAT-TAKE-VALUES** — as originally filed (lane B, 2026-08-22):

**In short:** Our command-line tools share a helper that produces the *error
messages* for bad options ("invalid option -- 'q'"). It does not do the actual
*reading* of the command line. That was fine while every converted tool had only
on/off switches like `-r`, but `touch` is the first with an option that takes a
value (`-r FILE`, `--reference=FILE`), and those can be written four different
ways. `touch` therefore carries about forty lines of its own command-line reader.
The moment a second tool needs one, that code will be copied — and two copies
will drift apart on the fiddly cases.

### Where

- The shared helper: `userspace/coreutils/src/getopt.rs`. It has
  `Program::invalid_option`, `unrecognized_option`, `short_missing_argument`,
  `long_missing_argument`, `argmatch`, `resolve_long`, and the `Takes` enum
  (`Nothing` / `Required` / `Optional`) — everything needed to *describe* an
  option table and to *complain* about it. There is no loop that walks argv.
- The private copy: `userspace/coreutils/src/bin/touch.rs` → `Cursor`,
  `parse_args`, `parse_long`, `parse_cluster`, `short_takes_argument`.

### The four spellings that have to be got right

For an option taking a value, all of these are the same thing, and GNU accepts
all four:

```
-r FILE        --reference FILE
-rFILE         --reference=FILE
```

Plus the cases that are easy to get subtly wrong, each of which `touch` has a
test for:

| Input | Correct behaviour |
|---|---|
| `-cr ref f` | bundling continues up to the value-taking option, which then eats the *next* argv word |
| `-r` at the end | `option requires an argument -- 'r'` |
| `--reference` at the end | `option '--reference' requires an argument` |
| `--no-create=x` | refused: the option takes no value |
| `--time=` | an *empty* value, which then fails `argmatch` and lists the valid words |
| `-d 2001-01-01` | the option is refused, but its value must still be *consumed*, or `2001-01-01` becomes a file name |

That last row is the one worth calling out: an option this crate does not
implement still has to be parsed exactly like one it does, or the refusal turns
into a silent file operation on the argument.

### Why it was not lifted into `getopt` immediately

Deliberate, not an oversight. An API designed from a single caller is one the
second caller has to fight, and the case that will actually shape the design —
`Takes::Optional`, where `--color` and `--color=never` differ but `--color never`
is *not* the option's value — has no caller at all yet. `touch` needs only
`Nothing` and `Required`, so a `getopt` driver written now would be guessing at
the third.

### What the correct fix looks like

When a **second** bin needs an option argument, lift it then, with two callers
in hand to check the shape against. The natural signature, given what is already
in `getopt.rs`:

```rust
impl Program {
    pub fn parse<'a>(
        &self,
        argv: &'a [OsString],
        shorts: &str,              // GNU's getopt string, e.g. "acd:fhmr:t:"
        longs: &[(&str, Takes)],
    ) -> Result<Vec<Opt<'a>>, Error>;
}
```

...yielding a flat sequence of `Opt::Short(u8, Option<OsString>)`,
`Opt::Long(&str, Option<OsString>)` and `Opt::Operand(&OsString)`, so each bin
keeps its own `match` over that sequence and only the walking is shared. Move
`touch`'s tests for the table above with it — they are the specification.

Trigger: **the second bin that needs an option taking a value.** The remaining
argv-conversion backlog makes that near-certain (`ls`, `find`, `du`, `dd`,
`sed`, `tar` all have them), so this should not sit long.
