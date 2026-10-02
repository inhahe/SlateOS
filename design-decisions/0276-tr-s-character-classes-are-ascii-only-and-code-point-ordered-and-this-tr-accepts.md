## 276. `tr`'s character classes are ASCII-only and code-point-ordered -- and this `tr` accepts a class in SET2 that GNU refuses

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A
**Zone:** shell (`kernel/src/kshell.rs`)

**In short:** `tr` is the command that swaps or deletes characters --
`tr a-z A-Z` upper-cases text. POSIX lets you name a whole group of characters
instead of listing them: `[:digit:]` means "the digits", `[:space:]` means "the
whitespace". Our `tr` did not understand that spelling at all, so
`tr -d '[:space:]'` deleted the letters of the word *space* and left the actual
whitespace untouched -- a wrong answer that exits 0. Implementing it forces two
questions that POSIX deliberately leaves to the locale: *which* characters are
in a class, and in *what order*. This entry records that both answers are the C
locale's -- ASCII only, ascending code point -- and that one GNU restriction is
deliberately not copied.

### The decision

1. **A class contains only ASCII members** (`tr_class_members`, a filter over
   `0u8..=0x7f`). `[:alpha:]` is 52 letters, not every letter in Unicode.
2. **Members are produced in ascending code-point order**, and that order is
   part of the contract, not an implementation detail.
3. **`[=c=]`, the equivalence class, expands to `c` and nothing else** -- which
   is exactly what it means in the C locale.
4. **A class *is* accepted in SET2 in cases GNU rejects.** GNU refuses
   `tr 'abc' '[:upper:]'` with *"misaligned [:upper:] construct"* unless SET1 is
   the matching case class; we accept any SET1 and pair positionally.

### Why ASCII-only

- **It is what GNU does in the C locale**, which is the locale this shell has.
  A script that works under `LC_ALL=C` -- the way portable scripts are written
  -- gets the same answer here.
- **It keeps a class inside `tr_apply`'s byte-clean fast path.** That path
  builds a 256-entry byte table whenever every member of SET1 is ASCII, and is
  provably identical to the character path because UTF-8 is self-synchronising:
  no ASCII byte ever occurs inside a multi-byte sequence. A Unicode
  `[:alpha:]` would force every input through a decoder, and would make
  undecodable input -- which `tr` is otherwise defined on -- an error.
- **A Unicode reading would have to answer a question `tr` cannot express.**
  `tr '[:lower:]' '[:upper:]'` is not "upper-case the text"; it is a
  *positional* correspondence between two lists, member *n* of the first
  mapping to member *n* of the second. That is well defined for 26 and 26. It
  is not well defined for the lowercase and uppercase halves of Unicode, which
  are neither the same size nor in any useful position-for-position
  correspondence. An ASCII class is the reading under which the command's own
  semantics work at all.

Ordering follows from the same argument: ascending code point is what makes
`[:lower:]`'s 26th member `z` and `[:upper:]`'s 26th member `Z`. Any other
order silently re-keys the map.

### Why accept what GNU refuses

GNU's "misaligned construct" check exists to catch a script whose two sets do
not line up. Ours cannot suffer the failure that check is guarding against:
a class here is always exactly the same fixed, ordered list, so the positional
pairing is always well defined -- `tr 'abc' '[:upper:]'` maps `a`→`A`, `b`→`B`,
`c`→`C`, which is the only thing it could sensibly mean.

The safety argument is the direction of the divergence. **Accepting more than
GNU cannot change the meaning of a script that already works under GNU** --
every such script is, by construction, one GNU did not reject. The reverse
divergence (refusing something GNU accepts) breaks working scripts, and the
much worse one (answering differently) is the silent-guess class this whole
series exists to remove. Being more permissive is the one direction that costs
nothing already-written.

Two GNU restrictions *are* kept, because both catch a real ambiguity rather
than a misalignment:

| Kept restriction | Why |
|---|---|
| Only `[:upper:]`/`[:lower:]` may appear in SET2 | The others have no useful positional meaning as a replacement; `[:punct:]` as a target is far more likely a typo than an intent. |
| `[=c=]` may not appear in SET2 | It denotes a *set* of equivalent characters, so as a replacement it names no single one. That it happens to be a singleton in the C locale is a property of the locale, not of the syntax. |

### Alternatives rejected

- **Unicode classes.** Rejected above: it breaks the fast path, makes
  undecodable input an error, and leaves `tr '[:lower:]' '[:upper:]'`
  undefined.
- **Refuse classes in SET2 entirely**, avoiding the alignment question. Rejected
  because `tr '[:lower:]' '[:upper:]'` is the single most common `tr` invocation
  there is.
- **Copy GNU's misalignment check verbatim.** Rejected because the condition it
  tests -- that the two sets might not correspond -- cannot arise here, so the
  check would reject only correct programs.
- **Treat a bad class name as literal characters.** Rejected in the code as
  well as here: `[:alhpa:]` has the *shape* of a construct with a bad name, so
  it is an error, while `[abc]` is not a construct at all and is five literal
  characters. Collapsing the two is precisely how a typo becomes a confident
  wrong answer -- `tr -d '[:alhpa:]'` would delete brackets, colons and the
  letters of the typo.
