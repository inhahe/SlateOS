## TD-C-THE-HTTP-CLIENT-CANNOT-MAKE-A-REQUEST — **WITHDRAWN, THE CLAIM WAS FALSE**

**Date:** 2026-09-08. **Lane:** C. **Withdrawn the same hour it was written.**

**The claim was that `net/httpclient` has no transport and nothing in the OS
can fetch anything. The second half is simply wrong.** `userspace/pkg` depends
on this crate (`Cargo.toml` line 19), builds requests with it, and carries them
over its own `http_roundtrip` — forty lines of `std::net::TcpStream` with read
and write timeouts. The package manager fetches. The module doc sentence I
"corrected" — *"used by the package manager and other applications for network
fetching"* — was accurate, and has been restored.

**How the mistake was made, because the mechanism is worth more than the
retraction.** The evidence offered was:

```
grep -rln httpclient --include=*.toml pkg/ apps/ net/   →  only net/httpclient
grep -rn "TcpStream" --include=*.rs pkg/                 →  nothing
```

**There is no `pkg/` directory.** It is `userspace/pkg`. Both greps searched a
path that does not exist, returned nothing, and that nothing was read as *"no
crate depends on it"* and *"the package manager has no socket code"*.

An empty result from a path that does not exist is byte-for-byte identical to
an empty result from a path with no matches. `grep -r` does print
`No such file or directory` to stderr — and the pipeline swallowed it, because
the habit of this session has been `2>/dev/null` on tree-wide greps to keep
permission noise out of the output.

**The rule that follows:** a negative grep result is evidence of nothing until
the path is known to exist. When a search is about to become a *claim*, confirm
the haystack first — `ls -d` the directories, or check the pattern matches
something known to be there. This is the second time today a negative result
misled me: the palette survey counted declarations and reported "0 left" three
times while sixteen, then five, then more applications were outstanding.

**What survives of the finding**, much narrower and not a bug: the transport is
the *caller's* by design, and exactly one caller has written one. A second —
the DynDNS updater in `apps/settings/src/remote.rs`, which is specified in
`roadmap-detailed.md` line 2531 — will need either its own copy of
`http_roundtrip` or `pkg`'s lifted somewhere both can reach. That is a genuine
question about where a shared transport lives, and it is recorded in the
crate's own module docs where the next person to need one will read it.
