## 1116. iconv runs glibc's conversion steps, ported as they are -- quirks and all, but not three bugs

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** A conversion can go wrong in small ways -- a byte that is not
valid, a character the other side cannot write, an output buffer that fills
-- and glibc's `iconv` answers each with a particular error and a particular
place to stop, which programs rely on to resume, fall back or report. Rather
than write a converter of our own and chase glibc's answers case by case,
this libc now converts the way glibc does, in the same steps and the same
order, so the answers agree by construction. Where glibc's answers are plain
bugs that damage text, they are not copied.

**What is ported** (`posix/src/iconv.rs`, from glibc 2.39's `iconv/` and
`iconvdata/`):

1. Each character set is one step to or from glibc's internal form (UCS-4 in
   the machine's order, which is also `WCHAR_T`). A conversion is two steps
   through an 8160-character buffer (`GCONV_NCHAR_GOAL`), or one when either
   side is `WCHAR_T`; `WCHAR_T` to `WCHAR_T` is refused, as glibc has no step
   for it.
2. Each step's loop is its glibc module's -- `loop.c`'s checks for input and
   room, or the UCS-4 modules' bulk loops -- including where the modules
   disagree: which of a full output and a cut-off input is reported first,
   and whether `//IGNORE`'s skip fails the conversion (`UCS-4`'s and reversed
   `UCS-2`'s do not).
3. The rounds are `skeleton.c`'s: where the target stops short, the round is
   decoded again into only what it took, so the input stops where the output
   did; a round that filled the buffer goes on; the call's end is the
   target's if it stopped, else the source's.

That brings glibc's quirks with it -- the ones that change an answer, not
the text: `//IGNORE` reports what the source skipped only if it was in the
last 8160 characters; what the target skips ends the call after its round;
a substitution clears the source's skip for its round. Each is probed and
pinned in the module's tests.

**What is not:** three glibc bugs, each of which damages text.

- glibc's reset clears "a mark was read" but not the byte order the mark
  set, so after a big-endian stream the next is read big-endian, marked
  little-endian or not -- `iconv -f UTF-16 a b` misreads `b`. The reset here
  clears both, as POSIX's "the initial state" means.
- glibc's `//TRANSLIT` converts a substitute by re-entering the target step,
  which writes the byte-order mark again, before every substitute of the
  first round: U+1F600 into `UNICODE//TRANSLIT` is `FF FE FF FE 3F 00`. Here
  the substitute is written by the step's encoder, and the mark once.
- When the call that reads a mark then has no room, glibc undoes its count of
  calls, and the next call reads the next two bytes as a mark again -- a
  leading U+FEFF is lost. Here the mark is read once.

**Descriptors.** glibc's `iconv_t` is a pointer and it crashes on a bad one.
Here it is a handle -- slot, generation and a tag -- into a table under a
spin lock (§301), so a closed, forged or `-1` descriptor is `EBADF`, at the
price of a lock around each lookup; the conversion itself runs outside it.

| Alternative | For | Against |
|---|---|---|
| **glibc's steps, ported (chosen)** | agreement by construction, for every error and every stopping point; a new character set is a decoder and an encoder, not a new set of cases | more code; carries glibc's quirks, which a cleaner converter would not have |
| One pass, matched to glibc case by case (the module until this entry) | smaller and simpler | each character set multiplies the cases; reading glibc's source beside the probes found five places the old three already disagreed |
| The port, bugs included | total fidelity | text damaged where glibc damages it: a stream misread after a reset, U+FEFF inserted before substitutes, U+FEFF lost |
| musl's `iconv` | small; no per-descriptor buffer | musl's answers: no transliteration table, UTF-16 big-endian without a mark, different errors -- and the programs here are written against glibc's |

**How to reverse.** Each bug's fix is local: the reset clears `from_swap`
(`Descriptor::reset`), the substitute goes through `encode_char` rather than
the step (`transliterate`), and `source_order` settles the order once. To
drop a quirk, change the step that has it; the tests name glibc's answer
beside each.
