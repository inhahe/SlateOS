## `fmt` filled lines greedily, so its breaks were not GNU's; it is GNU's now (lane B, 2026-10-02) — **FIXED** 2026-10-02

**In short:** `userspace/fmt` was a hand-written "simple text formatter". It
read argv as `String` (a file name that was not UTF-8 killed it before its
first statement), and it filled each line as full as it would go, where GNU
chooses all of a paragraph's breaks at once by minimising a cost -- so on any
paragraph of more than a line the output differed. It is replaced by
`coreutils/src/bin/fmt.rs`, a port of GNU coreutils 9.4's `fmt.c`, and the
crate is deleted (§1005). `scripts/fmt-diff.sh`: 187 cases, none differing
from GNU's -- most of them real prose (coreutils' README, NEWS, manual and the
comments of its sources) at many widths and goals in every mode, and three of
them 300 seeded random texts formatted with random options. A scratch run of
1500 more random texts agreed byte for byte as well.

**What the port had to reproduce, because it is observable:**

- the cost function, constant for constant: a line costs the square of its
  distance from the goal (93% of the width) and half the square of its
  difference from the next; breaks are cheaper after a sentence or other
  punctuation and before an opening bracket, dearer after a period that
  ends no sentence, before a sentence's last word and after its first; and
  no line but a single word reaches the width itself (`fmt -w 10` will not
  print a ten-column line);
- the 1000-word and 5000-byte paragraph limits: past either, the paragraph
  is printed to a cheap break and continued, its continuation's first line
  indented as the paragraph's first; a word over 5000 bytes is printed raw;
- one tab anywhere in a file's white space switches the output to tabs for
  the rest of that file;
- `fmt DIR` says `fmt: read error` and nothing more -- upstream hands the
  name to a format string with no place for it -- and `fmt <&-` adds
  `closing standard input: Bad file descriptor`, which needed
  `stdfd::close_stdin` (and `stdfd::close`, for an input whose close fails).
