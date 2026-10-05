## 1143. The multibyte conversions keep a NULL `ps`'s state per function and per thread, refuse a sequence at the first byte that makes it impossible, and read a NULL `s` as C words it

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a program converting text a byte at a time may leave it to
the C library to remember a character half-read, by passing no state of its
own. This library keeps a separate memory for each function and for each
thread, where glibc keeps one per function for the whole program -- so two
threads doing this at once cannot mix up each other's characters. And it
refuses a malformed byte sequence as soon as no further byte could make it a
character, where glibc's `mbrtowc` waits for the sequence's end.

| Choice | Taken | glibc 2.39 | Why |
|---|---|---|---|
| a NULL `ps`'s state | per function, per thread: 13 states, 104 bytes of each thread's block (`posix/src/perthread.rs`) | per function, per process | POSIX lets these functions be unsafe for threads when `ps` is NULL, so both conform; per thread there is no race to be unsafe with -- and in Rust a process-wide static written from two threads is undefined behaviour, not merely a wrong answer. Only a program that begins a character in one thread and finishes it in another would see a difference. |
| when an invalid sequence is refused | at its first impossible byte, by Unicode's table 3-7 | `mbrtowc`: at the sequence's end; `c8rtomb`: at the first impossible byte | C's `(size_t)-2` means "incomplete (but potentially valid)", and `E0 80` is not potentially valid. A caller feeding one byte at a time sees -1 a call or two sooner; valid text is unaffected. |
| code points past U+10FFFF | refused, both ways | `mbrtowc` reads `F4 90 80 80`; `c32rtomb` writes the old five- and six-byte forms | RFC 3629; nothing here would read them back |
| a NULL `s` | exactly C's equivalence: `c16rtomb(NULL, ...)` after a lone high surrogate is `c16rtomb(buf, u'\0', ps)`, an encoding error; `mbrtoc16(NULL, NULL, 0, ps)` hands a low surrogate still to come to no one | `c16rtomb` answers 1 and forgets the surrogate; `mbrtoc16` writes through the NULL pointer and crashes | C's words, which glibc's own `c8rtomb` follows |

**Where:** `posix/src/wchar.rs` (`state_for`, `internal`, `decode`),
`posix/src/uchar.rs`; known-issues
D-POSIX-UCHAR-WAS-ASCII-AND-THE-STRING-CONVERSIONS-MISCOUNTED.
