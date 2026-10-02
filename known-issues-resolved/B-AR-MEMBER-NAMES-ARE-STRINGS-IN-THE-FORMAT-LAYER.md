## B-AR-MEMBER-NAMES-ARE-STRINGS-IN-THE-FORMAT-LAYER (lane B, 2026-09-14) -- FIXED; ELF symbol names are a separate layer and stay text

`ar` no longer dies on an operand that is not valid UTF-8, but it does not
handle one either: it **refuses**, naming the bytes, at `decode_operand`.

**Why the refusal is where it is.** The `ar` format stores a member name in a
16-byte header field, with longer names in the `//` table. Those are **bytes** —
a member name is not required to be UTF-8, and `ar` on any other system will
happily produce an archive containing one. This implementation carries
`name: String` in the header structs (`main.rs:110`, `:567`, `:577`) and
outward through `member_basename`, `find_member`, the symbol table and the
listing. So a name it cannot decode is one it cannot represent, and the
boundary is the honest place to say so.

**What it cost before:** nothing was refused, because `env::args()`'s iterator
is a literal `unwrap` and the process died first. The refusal is strictly
better and is not the end state.

**It is worse than "cannot represent" — it cannot READ, either.** Found
2026-09-14 while scoping the conversion, at `main.rs:227`:

```rust
let raw_name = std::str::from_utf8(&hdr_bytes[0..16])
    .map_err(|e| format!("invalid name field: {e}"))?
```

That `?` aborts the whole archive parse. So `ar t` on a **valid archive** —
one GNU `ar` produced, containing one member whose name holds a byte that is
not UTF-8 — fails outright with `invalid name field`, and every member in it
becomes unreachable. Not the member: the archive. This is not a limit of our
own output, it is a refusal to read other people's, and it is the strongest
argument for doing the conversion rather than leaving the boundary refusal in
place.

The structural markers do **not** need decoding to keep working: `//`, `/`,
`#1/N` and `/N` are ASCII by the format's definition, so they can be matched on
bytes and the 16-byte field never has to be text at all.

**The fix** is to carry member names as bytes through the format layer —
`name: Vec<u8>`, comparisons on bytes, and `escape_unprintable` at the
display sites (`t` listing, `v` output, diagnostics). It is a real piece of
work across ~12 functions in a 3,130-line file, which is why it is written
down rather than half-done: a partial conversion that decodes in one place
and not another would produce archives whose index disagrees with their
members.

**Reachability:** `ar` is one of only three argv-baseline binaries on the
image, so this one is worth doing, unlike most of that backlog.

**Done 2026-09-14 for `ar` itself.** `ArHeader.name` is `Vec<u8>`, and all
three fatal decodes are gone — the 16-byte field, the GNU `//` table entry, and
the BSD `#1/N` tail. The structural markers match on bytes, as the format
defines them, so nothing in the parser needs the name to be text. A test builds
an archive whose member name holds `0xE9`, parses it, and finds the member by
those bytes; reintroducing the first decode makes it fail with the original
`invalid name field`, which is how the test is known to measure something.

Display sites go through `escape_unprintable`, so an unprintable byte in a
member name cannot forge a line of `ar t` output. `ar x` writes the member out
under its own name as an `OsString` rather than any text form.

**And done for `ranlib` and `strip` too, 2026-09-14.** Both held their file-path
operands as `String`. They carry `OsString` now, and `decode_operand` — which
by then served only them — is **deleted** rather than left as a refusal nothing
calls. Its two tests went with it: they asserted a refusal that no longer
happens, and a test that cannot fail is worse than no test.

`strip -K` still takes its symbol names as text, deliberately. Those are ELF
symbol names matched against a symbol table this file carries as `String`, and
that layer — `ElfSection.name`, `ElfSymbol.name`, both from ELF string tables —
is a different question from the `ar` member name and is not converted here.
