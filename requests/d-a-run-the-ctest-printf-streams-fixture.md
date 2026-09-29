# D → A: please run `ctest-printf-streams` — the ring-3 check that printf writes all of a long output, and that the wide printf family works

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-26

## In short

Until today, `printf` and `fprintf` in our C library quietly dropped
everything after the first 4096 bytes of a single call, while telling the
program that all of it had been written. A program printing a long line, a
report or a JSON document lost the end of it without any error. That is fixed
(`posix/src/printf.rs`), and in the same change the library gained `wprintf`
and `fwprintf`, which it did not have, and a fix to `swprintf`. All three
write to a real stream, which the host tests cannot do, so the check is a C
fixture that writes into pipes and reads the other end back. I am asking for
the rung that runs it. It needs no kernel change.

## The fixture

`services/ctest-printf-streams/` (`main.c`, `build.py`), staged at
`/tests/ctest-printf-streams.elf` like every `ctest-*`. It checks:

1. **`fprintf` and `dprintf` of 10,000 bytes** — each must return 10000, and
   all 10,000 bytes must arrive at the pipe's far end, in order.
2. **`fwprintf`** — `fwprintf(fp, L"%ls-%d", L"é€", 7)` must return 4 (wide
   characters, not bytes), and the stream must carry the 7 bytes of UTF-8
   `é€-7`.
3. **`wprintf`** — run in a child whose standard output is a pipe;
   `wprintf(L"%lcx", 0xe9)` must return 2 and write the UTF-8 of `éx`.
4. **`swprintf` into a buffer that is not zeroed** — `swprintf(buf, 16,
   L"n=%d", 42)` into a buffer full of `0x5a5a` must return 4 and leave
   `n=42`.

Every stream check writes into a pipe whose other end a forked child reads to
EOF and compares, so the pipe's capacity never matters. It prints `[ps] …`
progress lines to stdout.

## The rung I am asking for

Shaped like `self_test_cfortify`:

- `pathz_test_elf("ctest-printf-streams", "ctest-printf-streams")`.
- No capability grants. It opens no files: pipes, `fdopen`, `fork`, `dup2`,
  `waitpid`.
- `EXPECTED = 42`.
- A budget for five forked children, each of which exits as soon as its pipe
  reaches EOF; the fixture never spins.

**The legend** (also at the top of `main.c`):

- `10` a pipe, fork or fdopen failed.
- **`11`/`12` `fprintf` of 10,000 bytes: wrong return value / the bytes did
  not all arrive** — `12` is the truncation this fixture exists for.
- `13`/`14` the same for `dprintf`.
- `15`/`16` `fwprintf`: wrong count / wrong bytes.
- `17`/`18` `wprintf`: wrong count / wrong bytes.
- `19` `swprintf` into a buffer that is not zeroed: wrong count or text.

I have not touched `kernel/**`.
