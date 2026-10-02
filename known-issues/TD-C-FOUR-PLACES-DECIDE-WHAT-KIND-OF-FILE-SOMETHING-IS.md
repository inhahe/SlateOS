## TD-C-FOUR-PLACES-DECIDE-WHAT-KIND-OF-FILE-SOMETHING-IS

**Date:** 2026-09-16. **Lane:** C.
**Where:** `gui/toolkit/src/filetypes.rs` (the one with a real table),
`apps/filesearch/src/main.rs` (~315, ~350), `apps/diskanalyzer/src/main.rs`
(~771), and until today `apps/fileassoc/src/main.rs`.

**In short:** four different parts of this system each decide, from the letters
after the dot, what kind of thing a file is — and each keeps its own list.
They already disagree. Add a new format to one and the others carry on not
knowing about it, with nothing failing to build and no error anywhere. Two of
the four were merged into one list today; the other two are recorded here
because merging them would quietly remove things a user can currently see.

**What is where.**

| place | what it decides | how |
|---|---|---|
| `guitk::filetypes` | extension -> kind, MIME, description, icon | a 97-entry table, the real one |
| `apps/fileassoc` | which types exist to associate | **fixed 2026-09-16** — derives from the table |
| `apps/filesearch` (~350) | the search facet a result falls under | **the one left** — its own `match`, its own 11-variant enum |
| `apps/diskanalyzer` (~771) | the colour a file gets in the usage map | **fixed 2026-09-16** — derives from the table |

**`diskanalyzer` done 2026-09-16, and it had already drifted.** Its list knew
`.tiff` and `.zst` but not `.mpg`, `.m4v`, `.vob` or `.psd`, so a disk full of
MPEG video drew in the fallback grey. That is what these lists produce when
they age: not an error, a picture that is quietly wrong. The colours stayed --
"video is blue" is a treemap policy, not a fact about files -- and only the
classification moved. Four extensions keep an explicit entry there (`exe`,
`dll`, `dylib`, `bin`), because the toolkit holds those out on purpose and
deriving without the exception would have taken a colour away from every
Windows binary on the disk.

**So `apps/filesearch` is the only one left**, and it is still blocked on the
`Font`/`Database` kinds, not on effort.

**Why the remaining two were not simply merged, which is the useful part.**
`apps/filesearch` has `Font` and `Database` facets. The toolkit files `.ttf`
and `.woff` under `System`, and does not know `.db` or `.sqlite` at all. So
deriving filesearch from the toolkit today would take two working search
filters away from the user and replace them with `Other` — a regression
delivered as a cleanup, which is the worst way to receive one. The duplicate
list is not redundant; it is *better informed* in two places and worse in the
rest.

`apps/diskanalyzer` maps extensions to palette roles rather than to a kind, so
the shape is not the same: it wants "video is blue", which is a colour policy,
not a fact about files. Its extension lists are what duplicate the table.

**Closed the no-decision half the same day.** Sixteen of the thirty were added
to `gui/toolkit/src/filetypes.rs`: the twelve media and text formats, then
`cab` and `lz4` (archives missed on the first pass) and `elf` and `so` -- this
system has a POSIX layer, so an ELF binary and a shared object are ours rather
than foreign. `.so` is filed `Library` rather than `Executable` because it is
loaded, not started. The table now holds 110 extensions, up from 94.

**Fourteen remain, and every one of them is waiting on a decision rather than
on effort:**

```
accdb app bin db dll dylib eot exe mdb msi raw sqlite sqlite3 wasm
```

* `exe dll msi app dylib` -- foreign executables. Pinned absent by
  `foreign_executables_are_absent_on_purpose`, which says in its own doc to
  delete it in the change that decides they belong.
* `db sqlite sqlite3 mdb accdb eot` -- need `Database` and `Font` kinds the
  enum does not have.
* `raw bin wasm` -- ambiguous. `raw` names camera images and raw byte dumps
  equally; `bin` is any binary blob; `wasm` is a module format with no runtime
  here. A single description for any of them would be a guess.

So the step that needed no judgement is done, and what is left is exactly the
part that needs somebody to choose. Deriving `filesearch` from the table is
still blocked on the middle group.

**Measured 2026-09-16, so step 1 is a lookup rather than an investigation.**
`filesearch` names 107 extensions; the toolkit's table holds 94; **30 are known
to `filesearch` and not to the table**:

```
accdb app bash bin cab db dll dylib elf eot exe fish lz4 m4v mdb mpeg mpg
msi properties ps1 psd raw so sqlite sqlite3 tex vob wasm yml zsh
```

They are not one kind of gap, which is why this is not a bulk import:

* **Plainly missing** — `mpg`, `mpeg`, `m4v`, `vob`, `psd`, `raw`, `yml`,
  `tex`, `bash`, `zsh`, `fish`, `ps1`, `properties`. Formats this system has
  every reason to recognise, absent for no reason anyone recorded.
* **Possibly deliberate** — `exe`, `dll`, `msi`, `app`, `dylib`. Foreign
  executables this OS does not run. Adding them to the table makes the file
  manager offer to classify something it can do nothing with, so the omission
  may be a decision rather than an oversight. It is not written down either
  way, which is the actual defect.
* **Ours and missing anyway** — `elf`, `so`, `wasm`, `bin`. A POSIX system
  ought to know an ELF binary and a shared object.
* **Needs the enum first** — `db`, `sqlite`, `sqlite3`, `mdb`, `accdb`, `eot`,
  and the font extensions, since `Font` and `Database` are `filesearch` facets
  the toolkit's sixteen kinds cannot express.

So the order matters: the first group can be added with no decision at all; the
second needs somebody to say whether this OS classifies formats it cannot open;
the fourth needs the enum. Do not derive `filesearch` from the table before
those land -- it would turn 30 working classifications into `Other`, and a
regression arriving as a cleanup is the worst way to receive one.

**The proper fix, in order:**

1. Add what filesearch knows and the table does not: a `Font` category (or a
   deliberate decision that fonts stay `System`), and the database extensions.
   This is a change to a shared enum used by `apps/explorer` and
   `apps/fileassoc`, so it is a design decision rather than a tidy-up — which
   is why it is written down instead of done in passing.
2. Then derive filesearch's facet from `category_from_extension`, mapping the
   toolkit's kinds to its facets the way `fileassoc::FileCategory::from_toolkit`
   does — an explicit total match, so a new toolkit kind fails to compile
   rather than landing silently in `Other`.
3. Then give diskanalyzer its colour policy over toolkit kinds instead of over
   its own extension lists.

**A fifth place was suspected and is not one.** `apps/explorer` has its own
nine-bucket `FileType` with its own icon glyphs, which has the shape of another
duplicate. It is not: `FileType::from_extension` already asks
`detect_from_extension` and maps the toolkit's sixteen kinds onto its nine,
exactly as `fileassoc::FileCategory::from_toolkit` now does. Its doc records
that it used to be a hard-coded list, "one of three in this application", and
why the enum survives the conversion: `is_text` separates a `.txt` from a
`.pdf`, both `Document` to the registry, and only one is something a text
thumbnail can be made of.

Worth saying plainly, because it changes what this entry is: the method
proposed above is not new. It is the conversion `apps/explorer` already went
through, which `apps/fileassoc` had missed. `filesearch` and `diskanalyzer` are
the two left.

**Still true, and wider than one function.** Counted 2026-09-16, callers
outside the module:

| function | callers |
|---|---|
| `detect_from_extension` | 2 |
| `category_from_extension` | 3 |
| `icon_for_extension` | **0** |
| `mime_for_extension` | **0** |
| `is_text_file` | **0** |
| `is_executable` | **0** |
| `parse_extension` | **0** |
| `detect_from_magic` | **0** |

Six of eight have no caller, and the reason is visible in the two that do:
consumers call `detect_from_extension` and then read the fields off the
`FileTypeInfo` -- `apps/explorer` uses `info.is_text` and `info.description`
directly. So the single-purpose accessors are a second API for what the general
one already returns, and everybody picked the general one. That is a shape to
decide about, not a defect: either they are convenience worth keeping for the
next caller, or they are surface to remove. They are cheap either way.

**`detect_from_magic` is not in that group and is the interesting one.** It
identifies a file from its first bytes, and nothing anywhere calls it. Every
classifier in this tree works from the extension alone, so a file with no
extension -- ordinary on a POSIX system -- is `Unknown` everywhere, while the
code to identify it properly is sitting written and tested in the toolkit. That
is the same shape as the five features in open-questions C-Q17, and it wants
the same decision. Wiring it has a real cost to weigh: it means reading the
first bytes of files during a directory listing, which the extension path does
not do.
