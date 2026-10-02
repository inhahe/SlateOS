## 721. `fetch` resolves names itself, requires an ASCII host but not an ASCII path, and refuses to let a URL choose a file name outside the current directory

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B
**Where:** `userspace/coreutils/src/bin/fetch.rs` — `net::Connection::connect`,
`ParsedUrl::parse`, `ParsedUrl::filename`.

**In short:** `fetch` is the "download this URL" utility. Rewriting it to stop
crashing on odd filenames turned up that it could not reach a web site at all —
it only understood numeric addresses like `93.184.215.14`, never names like
`example.com`. Fixing that forced four small choices about what to do with the
awkward cases: a name with accents in it, a name that resolves to several
addresses, a slow name lookup, and a URL that tries to name the file it will be
saved as. None of them has an answer that is simply right; each is written down
here with what a user would see either way.

### Decision 1: A URL's **host** must be ASCII; its **path** may be any bytes

*What changes:* `fetch http://café.example/` says
`non-ASCII host name (IDN is not supported)` and exits 3, while
`fetch http://example.com/café.txt` works and saves a file called `café.txt` —
or, with a path holding bytes that are not text at all, a file holding exactly
those bytes.

- **For:** The two halves of a URL genuinely differ. The path is opaque octets
  that we hand to the server and, with `-O`, to `open` — treating it as bytes
  is not a compromise, it is the correct model, and it is the whole point of
  the argv-UTF-8 conversion. The host is not opaque: it must be turned into a
  question for a resolver, and a resolver takes a *name*, which for a non-ASCII
  label means Punycode (the `xn--` encoding that spells `café` as `xn--caf-dma`)
  — an implementation of IDNA, including its normalisation and its per-script
  validity rules, that is far larger than this utility and belongs in a library
  the OS does not yet have. Refusing is honest; the alternative is passing the
  raw UTF-8 to the platform resolver and getting a failure whose message blames
  the network.
- **Against:** A user who pastes an internationalised URL from a browser gets a
  refusal for something the browser plainly manages. And the split is a little
  subtle to explain: the same accented characters are fine four characters to
  the right.
- **Why the "for" wins:** The refusal names its own reason (`IDN is not
  supported`) and so is a request for a feature rather than a mystery. A
  silently-mangled lookup would be neither.

### Decision 2: Every resolved address is tried, and the error reported is the last one

*What changes:* On a machine with no IPv6 route, `fetch http://<dual-stack>/`
succeeds via IPv4 instead of failing on the AAAA record that sorted first.

- **For:** A name commonly resolves to several addresses precisely so that a
  client can fall back, and this is what every other HTTP client does. Trying
  only the first would make `fetch` fail on hosts that work everywhere else,
  for a reason invisible to the user.
- **Against:** With a long address list and an unreachable host, the wait is
  `--timeout` multiplied by the number of addresses, not `--timeout`. And
  reporting the *last* error can be misleading: if IPv6 fails instantly with
  `ENETUNREACH` and IPv4 then times out, the user sees the timeout — arguably
  the less informative of the two.
- **Why the "for" wins:** Connecting to a host that works matters more than the
  precision of the message when none of them does; and the last error is in
  fact usually the informative one, because the address family that got
  furthest is the one that took longest to fail.

### Decision 3: `--timeout` does not cover name resolution

*What changes:* `fetch --timeout 2 http://slow-to-resolve/` can block for
however long the platform resolver blocks — typically 5–30 s — before the two
seconds it promised ever start.

- **For:** `std::net::ToSocketAddrs` is a blocking call into the platform
  resolver with no deadline parameter, and there is no portable way to bound it
  short of doing the lookup on a thread and abandoning it. Abandoning a
  blocked resolver thread leaks it for the life of the process; joining it
  defeats the purpose. Neither is something to bolt onto one utility.
- **Against:** A flag named `--timeout` that does not bound the total wait is a
  flag that lies, and the case it fails to cover — an unreachable DNS server —
  is one of the commonest ways a fetch hangs.
- **Why the "for" wins:** The gap is documented at the call site and in
  `known-issues.md` rather than papered over, and the real fix is a resolver of
  our own, which this OS needs anyway once `net/` serves userspace. A
  thread-per-lookup hack in one binary would make that fix harder to land, not
  easier. (This is the "document the deferral with its trigger" case from
  `CLAUDE.md`, not a quick hack: the trigger is `net/`'s userspace resolver.)

### Decision 4: `-O` does not percent-decode, but does refuse `.` and `..`

*What changes:* `fetch -O http://x/caf%C3%A9.txt` writes a file literally named
`caf%C3%A9.txt` (curl's behaviour, not wget's), and
`fetch -O http://x/a/b/..` refuses instead of writing to the parent directory.

- **For:** Not decoding is what curl does, and the reason is that decoding lets
  the *server's* URL choose bytes in a local file name — `%2F` would introduce a
  path separator, `%00` a terminator. Refusing dot segments closes the same
  door from the other side: a URL ending in `..` would otherwise make `-O` write
  outside the directory the user ran the command in, which is not something a
  download tool should ever do on the say-so of a redirect.
- **Against:** The file name is then not the name the user sees in their
  browser, which is mildly surprising for the common accented case; and a
  literal `%` in a file name is ugly. wget decodes, so a user coming from wget
  will notice.
- **Why the "for" wins:** The surprise is cosmetic and the alternative is a
  path-traversal primitive driven by a remote server. Note that the refusal is
  needed *because* we do not decode — with decoding, the check would have to be
  redone after decoding, and getting that order wrong is exactly how these bugs
  ship.
