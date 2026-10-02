## 935. An undecodable 8.3 name is escaped, not guessed; the code page is a per-mount override

**Date:** 2026-09-13 · **Decided by:** Operator · **Lane:** A

Answering A-Q12 with options 3 and 4 together, which is what Claude recommended.

**In short:** FAT stores short filenames as raw bytes and does not record which
alphabet they are in. We currently guess UTF-8, fail for every accented name,
and print `????????` -- the same eight characters for *every* such file, so two
different names become one and only the first is reachable. From now on an
undecodable 8.3 name is shown as visible escapes (`r\\xe9sum\\xe9`), and a mount
may be told its code page explicitly.

**Why escapes are the default rather than code page 437.** An escape is
reversible and injective: it says which byte it was, so two different names stay
two different names. 437 gives *an* answer for every byte, so a Greek or
Japanese disk reads as confident nonsense -- a wrong answer where there is now a
visibly odd one. In the absence of information the correct output is the bytes,
not a guess dressed as a name.

**Why the code page is offered at all.** Someone who knows their disk came from
a US or Western-European machine can have correct names, which escapes cannot
give them. It is an optimisation for when the information exists, which is why
it is a per-mount option and not a global default.

**What this does not change.** The lookup guard from 2026-09-12 stays: a name
that does not decode is still never *matched* against a fabricated string, so
looking for one such file can never return another. Escapes make the collision
visible; the guard is what stops it being harmful.

**Observed, not predicted.** A boot self-test builds two directory entries
differing only in the first 8.3 byte (0xE9 vs 0xEF) and shows the kernel
rendering both as `"????????.TXT"`. That is the collision this decision ends.
