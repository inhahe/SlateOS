### [B] TD-B-LS-ACCEPTS-HYPERLINK-WITHOUT-EMITTING-IT — 2026-08-22 — OPEN (tech debt)
**Status:** OPEN — re-verified 2026-09-24: `print_hyperlink` is still read only to cancel `--dired`.

**What it is.** `ls --hyperlink[=WHEN]` parses, validates its argument and sets
`Settings::print_hyperlink`, and then nothing reads it. GNU wraps each name in
an OSC 8 escape — `\033]8;;file://HOST/ABS/PATH\a` before it and `\033]8;;\a`
after — so a terminal that understands the sequence turns the listing into
links. We print the bare names.

The flag is not inert, though: it already suppresses `--dired`, because
upstream's `dired` is `dired && format == long_format && !print_hyperlink`.
So `ls -l --dired --hyperlink=always` correctly prints no `//DIRED//` line on
both sides, which is the one observable thing the flag does here.

**How to see it.** `scripts/ls-diff.sh`, the two cases marked
`!hyperlinks are not implemented`:

```text
$ ls --hyperlink=always t | cat -v
GNU:   ^[]8;;file:///tmp/…/t/a^Ga^[]8;;^G  …
ours:  a  …
```

**Why it was not done with the colour half.** The escape carries an *absolute*
path and a hostname, and both are missing pieces rather than missing code:

- The path is upstream's `f->absolute_name`, built by gnulib's `canonicalize_filename_mode
  (name, CAN_MISSING)` when the operand is relative. We have no `canonicalize`
  and no `getcwd` wrapper in `userspace/coreutils`; the nearest thing is
  `coreutils::pathname`, which is pure string work and deliberately does not
  touch the filesystem.
- The hostname is `xgethostname()`, i.e. `gethostname(2)`. SlateOS has no
  syscall for it yet and no `/etc/hostname` convention settled, so there is
  nothing to read.

Doing it without those two would mean emitting a link that resolves to the
wrong file, which is worse than emitting none: a terminal would offer to open
something the user did not list.

**What the proper fix looks like.** Three pieces, in order:

1. A `getcwd` + `canonicalize` pair. The natural home is a small
   `userspace/canonpath` crate (the same shape as `pwdb` and `localtime`),
   because `readlink -f`, `realpath`, `pwd -P` and `find` all want it and none
   of them has it either.
2. A hostname source — a syscall, or `/etc/hostname` with the empty string as
   the documented fallback (GNU's `xgethostname` cannot fail, and an empty host
   in a `file://` URI means "this machine", which is exactly right).
3. In `ls.rs`: set `FileInfo::absolute_name` in `gobble_file` when
   `print_hyperlink` is on, and in `print_name_with_quoting` emit the two
   escapes around the name. The `skip_quotes` branch goes with it — when outer
   quotes are being aligned, upstream puts the opening quote *outside* the link
   so the underline starts at the name — and both escapes must go through
   `Out::put_str` so that they do not advance `Out::pos`, for the same reason
   the colour escapes do not.

**Trigger:** do it when `userspace/canonpath` exists for another reason, or
when a terminal in the image starts honouring OSC 8.

**Where it shows.** `scripts/ls-diff.sh`, cases `ls --hyperlink=always t` and
`ls --hyperlink=always -l t`, both marked `!`.
