## 701. `tar` renders every name in gnulib's `escape` style, not the tree's usual `quotef` — including its listings, which used to print raw bytes

**Date:** 2026-08-29
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** When a program has to print the name of a file, it has to decide
what to do about names that contain awkward things — a newline, a space, a byte
that is not a letter in any alphabet. Almost every utility in this tree wraps
such a name in shell quotes, so `my notes.txt` prints as `'my notes.txt'`. GNU
`tar` does something different: it puts a backslash escape in place of the
awkward byte and adds no quotes at all, so the same name prints as `my
notes.txt` and a name holding a raw byte 0xE9 prints as `caf\351.txt`. This
section is the decision to follow `tar` rather than the tree, and — the part
that is a real change rather than a matter of taste — to apply it to `tar -t`
and `tar -cv`/`-xv` output as well as to error messages.

**Glossary.** *Quoting style* — the rule a program uses for turning a file name
into displayable text. *`quotef`* — the tree's usual one (gnulib's
`shell-escape`: shell quotes, `$'\n'` for a newline). *`escape`* — gnulib's
other one (C escapes, octal for anything that decodes to no character, no
quotes). *Listing* — the lines `tar -t` prints, one per member of the archive.

### The measurement

GNU tar calls `set_quoting_style (NULL, escape_quoting_style)` once at startup,
which changes the *default* for every later `quotearg` call. The effect is that
its diagnostics and its listings agree with each other. Measured against GNU
tar 1.35 (`/tmp/gnuref/tar-quote.sh`, `tar-quote2.sh`):

| name | `tar -t` prints | `tar: … Not found in archive` says |
|---|---|---|
| `with space` | `with space` | `with space` |
| `it's` | `it's` | `it's` |
| `with<TAB>tab` | `with\ttab` | `with\ttab` |
| `caf<0xE9>` (invalid UTF-8) | `caf\351` | `caf\351` |
| `café` (valid UTF-8) | `café` | `café` |
| `back\slash` | `back\\slash` | `back\\slash` |

`--show-defaults` confirms it in one line: `--quoting-style=escape`. The rule
is locale-sensitive at the last row but one — under `LC_ALL=C` even `café`
becomes `caf\303\251`, because no byte over 0x7F is part of a character there.
We implement the UTF-8 answer, which is the one this system's paths are.

### What changed on our side

Two things, and only the second is a behaviour change a user would notice:

1. **Diagnostics** moved from `quotef` to `escape`. Cosmetic; the wording was
   already GNU's.
2. **Listings and `-v` output** moved from *raw bytes* to `escape`. The
   previous code wrote the member name's bytes to stdout unaltered, with a
   comment defending it: a listing is output rather than a message, and `tar
   -tf a.tar` ought to be feedable to whatever extracts the archive, so the
   bytes should survive.

That defence does not hold up, for two reasons found by measuring rather than
by reasoning about it:

- **GNU's output is not feedable back either.** It escapes, so a round trip
  through `tar -t | xargs tar -x` is already broken for exactly the names the
  raw version was protecting. Being the only tar that preserves the bytes buys
  nothing, because nothing downstream expects it.
- **A raw name can forge lines.** A member called `a` + newline + `b` printed
  raw makes `tar -t` report two members, the second of which does not exist.
  That is the same class of problem the quoting module exists to prevent on
  stderr; there is no reason stdout should be exempt, and the argument that "it
  is content, like `cat`" is wrong — `cat` prints a file's *contents*, whereas
  a listing is tar's own structured report *about* a file.

### The cost, stated plainly

The rendering is not reversible. A member whose name really does contain a
backslash lists with it doubled, and there is no flag here (yet) to turn that
off — GNU has `--quoting-style=literal` for it. If a caller ever needs the
bytes, the fix is to add that option, not to change the default; the default
has to be the safe one because it is what an unattended script gets.

### Why this did not become an `open-questions.md` entry

There is a genuine tradeoff (round-trippable output vs. forgery-proof output),
but it is not an open one: the reference implementation resolved it decades
ago, in the direction that is also the safe one, and `tar-diff.sh` measures the
difference on every run. Deviating would mean carrying a difference from GNU
forever in exchange for a round trip that no consumer performs.

### Consequence for the `quoting` crate

`escape` and `escape_os` are new public functions there — gnulib's
`quotearg_style (escape_quoting_style, …)` — sitting beside `quotef`/`quoteaf`.
They return `String` rather than `Vec<u8>` (which `Style::Escape` returns)
because the style's output is always valid UTF-8 by construction, and a caller
that must format it into a message should not have to prove that again.
`ls -b`, `du` and `df` select the same style upstream and can use them.
