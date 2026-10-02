## `B-tar-READ-EVERY-PATH-AS-UTF-8` (lane B, 2026-08-29) -- **FIXED 2026-08-29**

**In short:** on this OS a filename may hold any byte except `/` and NUL, so a
file can perfectly legally be called `café.txt` in Latin-1 -- a name that is not
valid UTF-8. `tar` assumed every name was UTF-8 in three places, and each one
lost data rather than complaining: naming such a file on the command line
**aborted the program**; archiving a directory containing one **stored it under
a different name**, so the archive was not a copy of the tree; and extracting an
archive that contained one **wrote it to disk under a different name**. All
three are fixed -- `tar` now carries names as bytes from argv to the syscall and
back.

### What it actually did (measured, 2026-08-29, against the commit before the fix)

Fixture: `src/plain.txt`, `src/café.txt` (the `é` is the single byte `\351`),
`src/sub/<FF><FE>.bin`. `\351` alone is not valid UTF-8; `\377` can never appear
in UTF-8 at all.

| | before | after |
|---|---|---|
| `tar -cf x.tar 'src/café.txt'` | **panic, SIGABRT (rc 134)**: `called Result::unwrap() on an Err value: "src/caf\xE9.txt"` | archives it, rc 0 |
| `tar -cf a.tar src`, then `tar -tf a.tar` **with GNU tar** | GNU reads back `src/caf\357\277\275.txt` -- U+FFFD, three bytes, **stored in the archive** | GNU reads back `src/caf\351.txt` |
| our `tar -xf` of a **GNU-made** archive of that tree | writes `caf<U+FFFD>.txt` and `<U+FFFD><U+FFFD>.bin` to disk | writes the original names |
| `diff -r src extracted` | `Only in src: café.txt` / `Only in o/src: caf<U+FFFD>.txt` | `IDENTICAL` |

Two things are worth separating out of that table.

**The corruption was at *create* time, not only at display time.** It is easy to
read "lossy decode" as a printing problem. It was not: the U+FFFD went into the
100-byte `name` field of the header, which is why *GNU* tar reads the wrong name
out of our archive. An archive made by the old binary is permanently wrong and
no extractor can recover the name.

**It was not reversible even in principle.** `<FF><FE>.bin` -- two bytes --
became six, one U+FFFD per bad byte, and distinct bad bytes all collapse onto
the same replacement character. There is no mapping back.

### The three sites

1. **`main`/`parse_args`** took `env::args()` (`Vec<String>`), whose iterator
   `unwrap`s on a non-UTF-8 argument -- hence the abort, which happened before
   `tar` had looked at anything. Now `env::args_os()` into `Vec<OsString>`, and
   the option scan walks a cluster **byte by byte** rather than `char` by
   `char`. `-f` and `-C` values and all operands are kept as `OsString` and
   handed to `File`/`set_current_dir`/`Path` untouched.
2. **`add_directory_recursive`** built each child's member name with
   `e.file_name().to_string_lossy().into_owned()`. This is the *create*-side
   corruption above: the lossy copy was both what went into the header and what
   the recursion descended with. Now `os_bytes(&e.file_name()).into_owned()`,
   collected as `Vec<u8>` and sorted by bytes (so the ordering stays stable for
   names that no longer survive a round trip).
3. **`extract_string`** ran the raw header field through
   `String::from_utf8_lossy`, and it is what read the `name` field, so *every*
   member name passed through it before anything else saw it. That is the
   *extract*- and *list*-side corruption. It is now `field_bytes`, which borrows
   the used part of the field and decodes nothing.

`sanitize_member_name` follows from (3) and is the part that mattered most for
security rather than fidelity: it took `&str`, which meant the `..` check was
being made about a string that had already been altered and was **not** the name
that would be written. It now takes and returns bytes, so the check and the
write are about the same thing. (No traversal escape was found through the old
arrangement -- U+FFFD cannot become `..` -- but "the guard inspects a different
value than the one used" is not a property worth relying on.)

Supporting changes: `TarHeader::set_name(&[u8])`; a `report_member(&[u8])`
helper that goes through `stdfd::diag_bytes`, because `diag!` routes through
`format!` and would have reintroduced a lossy decode for `-v`; `list_archive`
writing the name with `write_all` rather than `writeln!`, so `tar -tf` output
can be fed back to whatever extracts it; and `parse_octal` doing
`str::from_utf8().ok()` -- a field that is not ASCII is not a number either, so
it lands on the same 0 that `"garbage"` already did.

### One deliberate message change

`tar -Z` said `unknown option: -Z`. The byte is now rendered with `quoteaf`,
which always quotes, so keeping that shape would have produced the odd `-'Z'`.
It reads `invalid option -- 'Z'` instead, which is byte-for-byte what GNU tar
1.35 prints (measured: `tar -Q`). `quoteaf` and not `char::from` because the
byte comes from the command line, and rendering it raw would let a crafted
argument forge a line of `tar`'s stderr.

### Verification

- 51 host unit tests pass, up from 44. The seven new ones are the point of the
  change: a non-UTF-8 operand, `-f` value and `-C` value through `parse_args`;
  a non-UTF-8 name through `sanitize_member_name` both when it is legal and
  when it sits beside a `..`; `field_bytes` not decoding; and `list_archive`
  writing the raw bytes.
- End-to-end round trip on the fixture above: our archive extracted by **GNU**
  tar is `diff -r`-identical to the source tree, and a **GNU**-made archive
  extracted by ours is likewise identical. Both directions, because a bug that
  renamed on create *and* on extract could have cancelled itself out in a
  self-to-self test.
- `scripts/argv-utf8.py --check` drops from 10 findings to 9;
  `userspace/coreutils/src/bin/tar.rs:argv-as-string` is removed from the
  baseline ratchet.
- `cargo +nightly clippy -p coreutils --bin tar --tests` clean for the target.

### Why `tar` still does not use `coreutils::getopt`

Every other converted bin moved to `coreutils::getopt` at the same time as it
moved to bytes, so the exception needs stating: **GNU tar does not use
`getopt_long`.** It uses `argp`, and `tar --=x` reports **175** long options
including `--usage`, `--program-name` and `--HANG`. Two consequences:

- `scripts/getopt-ambiguity-check.py` auto-discovers any bin with a
  `const LONG_OPTIONS` table and diffs it *as a sequence* against
  `<util> --=x`. Adding a table to `tar` would therefore create an obligation to
  transcribe all 175 argp entries -- a table describing a parser we are not
  emulating.
- GNU tar's *old option format* (`tar cvf x.tar` -- a leading cluster with no
  dash) cannot be expressed in getopt at all, and this tar would need it to
  accept the spelling most scripts actually use.

So the byte conversion was done with tar's own parser. This is a scope
judgment, not a deferral of the bug: the argv corruption above is fixed either
way, and adopting getopt would not have fixed anything additional. Whether
this tar should grow long options and the old option format is a separate
question and is not currently blocking anything -- it accepts `-c/-x/-t/-v/-f/-C`
and treats `--anything` as a file operand, which is what it did before.
