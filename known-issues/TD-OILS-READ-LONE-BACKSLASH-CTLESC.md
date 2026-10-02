### TD-OILS-READ-LONE-BACKSLASH-CTLESC. `read` on input that is nothing but a trailing backslash leaves bash's internal `CTLESC` (`\001`) in the variable; osh leaves it empty — NOT-A-BUG (bash defect, deliberately not replicated) — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `read_record`, the non-`-r` reader.
osh drops a backslash that has nothing left to escape, so the record is empty.

**What:** with the whole input being one backslash and no newline,

```
$ printf '\' | { read v; printf '%s' "$v" | od -c; echo "len=${#v}"; }
0000000 001            <- bash 5.2.37
len=1
$ printf '\' | { read -a A; printf '<%s>' "${A[@]}" | od -c; }
0000000   < 001   >    <- bash 5.2.37
```

osh gives an empty value and length 0 in both. The exit status agrees (1 in
both, since no delimiter was seen), and every neighbouring case agrees exactly:
`printf 'a\' | read v` gives `a` in both, and `printf '\' | read -r v` gives a
single backslash in both.

**Why NOT a bug in osh:** `\001` is bash's `CTLESC`, the sentinel it prefixes to
a byte internally so that later quote-removal knows the byte was quoted. It is
supposed to be stripped before the value is stored. Here the backslash is
retained as `CTLESC` while the character it was to escape never arrives, so
nothing pairs with the sentinel and it escapes into the variable as data — a
sentinel leak, not a documented behaviour. Reproducing it would mean giving osh
a `CTLESC` representation purely to be able to leak it.

**Impact.** None. Reachable only from input consisting of exactly one backslash
with no terminator; `tests/corpus/read-processes-backslashes-as-it-reads.sh`
covers the surrounding cases and deliberately omits this one.
