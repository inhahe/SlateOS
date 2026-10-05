## §371 — `stat`'s `%N` is quoted in the built-in block too, so a file name cannot forge a line of `stat`'s own output

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)

**In short:** `stat somefile` prints a little report — `File:`, `Size:`,
`Access:` and so on, one field per line. GNU prints the file's name in that
report *exactly as it is*, with no quotes. A file name is allowed to contain a
newline on this system, so a file called `a<newline>Size: 0` makes GNU's report
contain a second, fake `Size:` line that nothing downstream can tell from the
real one. Our `stat` puts quotes around the name instead, so the fake line
cannot appear. The cost is that the ordinary report now shows `File: 'notes.txt'`
with quotes, where GNU shows `File: notes.txt`.

**The mechanism upstream, exactly.** `stat.c`'s `main` does:

```c
if (format) { if (strstr (format, "%N")) getenv_quoting_style (); format2 = format; }
else { format = default_format (fs, terse, false); ... }
```

`getenv_quoting_style` is what sets the style to `shell-escape-always` (that is
its `else` branch, taken when `QUOTING_STYLE` is unset — not a fallback to
literal). It is called **only** when the user passed `-c`/`--printf` *and* that
format contains the literal two characters `%N`. The built-in block contains
`%N` too, but never reaches this call, so it renders with gnulib's untouched
default, `literal_quoting_style`.

Two consequences, both measured against GNU 9.4 rather than reasoned about:

| Command | GNU 9.4 |
|---|---|
| `stat f` (name `a<LF>b`) | `  File: a` then a raw newline then `b` |
| `stat -c '%N' f` | `'a'$'\n''b'` |
| `stat -c '%.3N' f` | `a` — **unquoted**, because `strstr` does not find `%N` in `%.3N` |

The third row is the tell: this is not a policy, it is a `strstr` on the format
string. A directive that differs only by a width or a precision gets a
different quoting style, which no one designed.

**The decision.** `%N` is rendered through `QUOTING_STYLE` (defaulting to
`shell-escape-always`) everywhere it appears — in `-c`, in `--printf`, in the
built-in human-readable block, and with any width or precision attached.

**For:**

* The human-readable block is the only output here that a person reads
  line-by-line, and it is the only one where a forged line does damage. A name
  is attacker-chosen input in every case that matters: a tarball, a download
  directory, a shared `/tmp`. `design.txt` permits every byte but `/` and NUL
  in a name, so this is not a hypothetical on this OS.
* It removes an inconsistency rather than adding one. Under this rule `%N`
  means one thing; under upstream's it means two, chosen by a substring search.
* `%n` still exists and is still raw. A caller that genuinely wants the
  unquoted bytes has always had the directive for it, and that is the directive
  scripts already use for machine consumption.
* `ls` on this system already quotes by default for the same reason, so the two
  agree about what a name looks like on a terminal.

**Against:**

* It is a visible difference from GNU in the *default* output, which is the one
  people paste into bug reports and the one a naive `stat f | grep File:`
  parses. `File: 'notes.txt'` is not what a manual page shows.
* A script that already parses the built-in block (rather than using `-c`, as
  it should) will see the quotes. This is the real cost, and it is why the
  divergence is documented in the module docs as well as here.
* Reproducing upstream bug-for-bug has value of its own: it is the property
  that makes "measure GNU, assert the measurement" a usable method, and every
  deliberate exception weakens it.

**Why the balance falls this way.** The against-side costs are cosmetic and
recoverable — `%n` is one character away. The for-side cost is a forged line in
a security-relevant report, which is not recoverable by the reader, because by
construction it is indistinguishable from a real one. Where a divergence trades
appearance for a forgery, appearance loses.
