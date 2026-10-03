### TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE — 2026-08-07 — ✅ RESOLVED 2026-08-07 by decision, not by code

**Resolved as a decision, and kept here at the operator's request.** The
question this entry raised was Q38, answered 2026-08-07: **osh is UTF-8-only**,
and the harness moves to `LC_ALL=C.UTF-8` so the reference bash agrees
(`design-decisions.md` §104). Nothing in osh changed — the fix was choosing
which bash to measure against. The survey below is deliberately left intact,
because the option that was *not* taken (making osh locale-aware, as bash is)
is a real piece of bash behaviour that osh now permanently does not model, and
a future change of mind should start from this list rather than from scratch.

**What osh now gives up, stated once, here.** A real bash under `LC_ALL=C` is
not reproducible by osh at all. Scripts that set `LC_ALL=C` for speed or
determinism — a common idiom — get different answers from osh than from bash on
every site listed under **Where** below. That axis of bash's behaviour is
untested by the corpus and will stay untested.

**If the decision is ever revisited,** the scope is the whole of **Where**:
a locale notion threaded through `bytes.rs` (today free functions with no
state) and read at every character-counting site. Note that the C locale is the
*easy* half — a non-UTF-8 *multibyte* locale (EUC-JP, Big5) would be far worse,
so the honest scope of a revisit is "C vs UTF-8", not "all locales".

**Where:** `scripts/osh-bash-diff.py` pins the locale for both shells; osh's
character handling is `userspace/oils/src/bytes.rs` (`char_count`, `char_slice`,
`char_at`) and its callers — `${#v}`, `${v:off:len}`, `${v^^}`/`${v,,}`,
`printf %q`, `\u`/`\U` in `escape.rs`, `select`'s `display_width`.

**What:** bash decides *per locale* whether a string is a sequence of bytes or
of characters — every multibyte site is behind `HANDLE_MULTIBYTE` and calls
`mbrlen`/`mbstate`. osh has no such switch: it always does UTF-8 character
semantics. The harness pins `LC_ALL=C` (deliberately — the comment there wants a
reproducible environment), so on any multibyte input osh is compared against a
bash doing *byte* semantics, which is a baseline osh was never built for.

This went unseen because no corpus case had ever put a multibyte string anywhere
that counts characters. The first one to do it —
`a-dollar-quote-inside-a-grouping-construct-is-translated-at-parse-time.sh`, via
a `printf '%-46s'` label containing `…` — failed immediately.

**Repro** (same bash binary, only `LC_ALL` differs):

```
LC_ALL=C       bash -c 's="a…b"; echo "${#s}" "${s:0:2}"'   ->  5  a\xe2
LC_ALL=C.UTF-8 bash -c 's="a…b"; echo "${#s}" "${s:0:2}"'   ->  3  a…
osh            -c 's="a…b"; echo "${#s}" "${s:0:2}"'        ->  3  a…
```

Also `${v^^}`: `aéb` is `AéB` under C (bash leaves the é alone, having no
character to upcase) but `AÉB` under UTF-8 and in osh.

And `printf %q` / `${v@Q}` on a byte that is no character, which is the sharpest
illustration of why this is a *fork* and not simply an osh bug. bash's
`ansic_shouldquote` (lib/sh/strtrans.c) sends any non-basic byte to
`ansic_wshouldquote`, which quotes when `mbstowcs` fails. Under UTF-8 `a\xffb`
does not decode, so bash writes `$'a\377b'`; under C every byte is a character
and `iswprint(0xff)` holds, so bash writes the raw `a\xffb`. osh writes the raw
form — which was *correct against the harness as it was configured* and wrong
against a UTF-8 bash. Q38 decided that the UTF-8 bash is the one that counts, so
this row is now a genuine osh bug: `printf %q` on `aÿb` should write
`$'aÿb'`. Tracked separately as
`TD-OILS-PERCENT-Q-WRITES-A-NON-CHARACTER-BYTE-RAW`.

**Not** in this class, though they look like it: `printf`'s field width and
`%c`. Those are byte-counted in *every* locale, because bash hands them to C
(`PF`, printf.def:124; `getchr`, printf.def:1165) and nothing on that path
decodes the string. They were osh bugs, are fixed, and are pinned by
`printfs-field-is-laid-out-in-bytes-not-characters.sh`, which is verified
identical under both locales.

**What was done:** the harness pins `LC_ALL=C.UTF-8`
(`scripts/osh-bash-diff.py`), which makes osh's existing behaviour the one being
measured. The former workaround — keeping multibyte strings out of
character-counting positions in corpus cases — is no longer needed, and such
cases are now worth writing: they pin the UTF-8 semantics osh has committed to.
