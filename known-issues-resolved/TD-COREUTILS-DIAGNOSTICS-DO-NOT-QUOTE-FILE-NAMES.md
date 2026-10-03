## TD-COREUTILS-DIAGNOSTICS-DO-NOT-QUOTE-FILE-NAMES (lane B, 2026-08-16) — **fixed 2026-08-16**

**In short:** When a coreutil complains about a file, it prints the file's name
raw. GNU wraps the name in quotes. That sounds cosmetic and mostly is — until
the name contains a space, a quote, or a newline, all of which our filesystem
allows (paths may hold any byte but `/` and NUL). A name with a newline in it
can then *forge a second line of output*, which is a real problem for anything
reading a tool's diagnostics.

Compare, for a file that does not exist:

```
GNU:   sort: cannot read: 'no such': No such file or directory
ours:  sort: cannot read: no such: No such file or directory
```

and for one whose name holds a tab:

```
GNU:   sort: cannot read: 'no'$'\t''such': No such file or directory
ours:  sort: cannot read: no<TAB>such: No such file or directory
```

**GNU uses two different styles and the difference is deliberate.** They come
from gnulib's `quotearg`:

| Helper | Style | Used for | `no such` becomes |
|---|---|---|---|
| `quote()` | `locale_quoting_style` (C locale) | option arguments, key specs, tab characters | `'no such'` |
| `quotef()` | `shell_escape_quoting_style` | file names | `'no such'`, but `no'such` → `"no'such"` and a tab → `'no'$'\t''such'` |

`quotef`'s output is *shell-pasteable*, which is the point: a user who sees a
diagnostic about a file wants to be able to copy the name into a command. That
is why the two styles differ on a name containing a `'`.

**Where it lives.** Everywhere. This is not a `sort` defect — `sort` was simply
the first tool measured against GNU closely enough to notice. Every one of the
85 coreutils binaries formats file names into diagnostics with a bare `{}` or
`to_string_lossy()`.

**The proper fix** is a shared `quotearg` module in
`userspace/coreutils/src/lib.rs` implementing both styles faithfully, plus a
sweep of the binaries to route file names through `quotef` and option arguments
through `quote`. It is mechanical but wide, and it wants its own differential
harness (a fixture directory of names holding a space, a quote, a backslash, a
tab, a newline and a high byte, run through both implementations) — because
this is exactly the class of defect where reading the code proves nothing.

**Fixed 2026-08-16.** `userspace/coreutils/src/quote.rs` implements all three
styles — the entry above says two, which was wrong, and the third was found by
measuring rather than by reading:

| Function | Used when | `abc` |
|---|---|---|
| `quotef` | the name **ends** the message (`wc: NAME: No such file`) | `abc` |
| `quoteaf` | the name sits **inside a sentence** (`rm: cannot remove 'NAME': …`) | `'abc'` |
| `quote` | option arguments and other non-file text | `'abc'` |

Which one applies is decided by the shape of the sentence, not by the utility.

The rules were not read out of gnulib; `scripts/quote-probe.py` drives GNU
`sort` and `head` under `LC_ALL=C` over every byte in every position that
matters plus an adversarial and a random corpus, and
`userspace/coreutils/tests/quotearg-gnu.txt` records what GNU printed — 8333
rows, all reproduced. Recall lost to measurement four separate times: the lone
`{` that is a bash reserved word, the `#` that is special only at the front of
a word, the `~` that is allowed inside double quotes *only* at the front (the
opposite rule), and gnulib's stray `''` prefix.

The sweep covers 51 binaries: names that end a message go through `quotef`,
names inside a sentence through `quoteaf` — including the ~19 sites that
hand-wrote `'{name}'`, which looked quoted and was not — and option arguments
through `quote`. `tests/diagnostics_quote_names.rs` reads the source and fails
on either shape reappearing, since the next unquoted diagnostic will look like
every other `eprintln!` in the tree.

**What is still open**, found while doing this and tracked separately: an
option *name* is still printed raw, in GNU as well as here —
`sort $'--fo\nsort: /etc/shadow: Permission denied'` forges a line out of
glibc's getopt. See `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`.
