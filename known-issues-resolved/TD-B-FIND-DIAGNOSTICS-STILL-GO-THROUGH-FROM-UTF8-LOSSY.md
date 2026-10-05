### TD-B-FIND-DIAGNOSTICS-STILL-GO-THROUGH-FROM-UTF8-LOSSY -- FIXED (2026-08-23)

**Where:** `userspace/coreutils/src/bin/find.rs`, `userspace/coreutils/src/bin/grep.rs`.

Fixed as described below, and pinned by six new harness cases. `grep -n
'from_utf8_lossy\| as char'` over both files now returns exactly one hit, in
`#[cfg(test)]` code, where the input is a literal the test wrote itself.

**What the sweep actually changed.** Twenty-six sites in `find.rs` moved from
`String::from_utf8_lossy` to `quote::escape_unprintable`, and both sites in
`grep.rs`'s `compile_patterns` did the same. Three sites did something other
than a substitution, because a substitution would have been wrong:

- `qmark` now uses `std::str::from_utf8`, not the lossy form. The question it
  asks is only *whether* the bytes decode; the lossy form answers that by
  building the corrupted string first and then reporting that it had to.
- The `-ok`/`-okdir` confirmation prompt writes bytes directly to a locked
  `io::Stderr` with `write_all`, exactly as upstream's `fprintf (stderr, "< %s
  ... %s > ?", ...)` does. This is the one diagnostic in the file whose sink is
  a byte stream with no `String` in the way, and it is also the one where being
  wrong is worst: a prompt showing a *different* path from the one about to be
  handed to the command asks the user to confirm the wrong thing.
- `chr` renders a NUL as nothing (which is what C's `%c` does once the message
  reaches a terminal) and every other byte through `escape_unprintable`.

**A second corruption class the sweep uncovered, which this entry did not
originally know about.** Three sites did not use `from_utf8_lossy` at all —
they wrote `c as char`. That is not a decode; Rust's `u8 as char` reinterprets
the byte as the Unicode scalar with the same number, i.e. as Latin-1, so byte
`0xFF` becomes U+00FF and is then *encoded back out* as the two bytes `0xC3
0xBF`. The differential harness caught it the moment a non-UTF-8 argument was
passed: GNU printed `M-^?` (one byte), we printed `M-CM-?` (two). The sites
were `parse_type_letters`' "Unknown argument to -type" and "Duplicate file
type", and `parse_size`' "invalid -size type"; all three now call `chr`.
`grep -rn ' as char'` over `find.rs` returns nothing.

**How it is pinned.** `scripts/find-diff.sh` gained six cases that pass a
`\377` byte as an option argument, a format directive and a predicate name:
`-type`, `-perm`, `-used`, `-size`, `-printf %\377` and `-\377`. Five of them
are marked as deliberate differences — we escape, GNU writes the byte raw —
and the sixth, `-perm`, is deliberately *not* marked and must keep passing: its
diagnostic does not echo the argument, so it is the control showing the marking
is about rendering and not about the parse. The harness now reports

    454 case(s): 444 passed, 0 differed, 10 differ on purpose, 0 unexpectedly agreed

and the GNU-against-itself control run (`OURS=/usr/bin/find`) reports nine of
the ten deliberate cases as `XPASS`, which is the shape that makes the marking
meaningful.

**What remains, and it is a real refactor rather than a leftover.** We escape
where GNU writes the byte raw. Being byte-identical means making `find`'s
`Fatal`/`errmsg` plumbing byte-typed, which means diagnostics can no longer be
built with `format!` and the change reaches the forty-odd other binaries that
share `errmsg`. That tradeoff — raw versus escaped, and why lossy is never
acceptable either way — is written up in `design-decisions.md` §369, and the
five marked harness cases are the exact list that should flip back to plain
passes if the plumbing is ever converted.
