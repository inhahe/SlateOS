## B-HOSTNAME-RESOLVES-THE-DOMAIN-WITHOUT-ETC-HOSTS (lane B, 2026-09-11) — FIXED 2026-10-01

`hostname -d` and `hostname -f` answer from the resolver's search domain and
never consult `/etc/hosts`. net-tools resolves through nsswitch, so on this host:

    /etc/hosts:  127.0.1.1  Logoplex3.localdomain  Logoplex3

    net-tools    hostname -d  ->  localdomain
                 hostname -f  ->  Logoplex3.localdomain
    ours         hostname -d  ->  attlocal.net
                 hostname -f  ->  Logoplex3.attlocal.net

**Both exit 0**, so a script asking for this machine's FQDN gets a confident
answer that disagrees with every other resolver user on the same box —
`getent hosts`, `ping`, anything using `gethostbyname`. On a machine whose
`/etc/hosts` is the *only* place the FQDN is written, which is the normal case
for a host without DNS registration, ours invents one from the DHCP search
domain instead.

13 of `coreutils`' 50 differences and 14 of the standalone's 54 are this one
cause, so it is the largest single family in the pair and survives the
retirement.

**The fix** is to resolve the name the way the C library does — `getaddrinfo`
with `AI_CANONNAME` on the result of `gethostname`, which is what net-tools
does — rather than reading `/etc/resolv.conf`'s `search` line. The distinction
matters beyond this program: the resolver's search list is for *completing
queries*, not for naming this host.

### BLOCKED, 2026-09-14 — our own libc does not implement `AI_CANONNAME`

The fix above names a mechanism this project does not have.
`posix/src/socket.rs:4589` says so outright, in the `getaddrinfo` result
constructor:

```rust
// Always NULL: we do not implement AI_CANONNAME.  If it is ever
// added, the name must live inside this same block (glibc does the
// same) — `freeaddrinfo` frees the node and nothing else.
ai_canonname: core::ptr::null_mut(),
```

So `getaddrinfo(…, AI_CANONNAME, …)` returned a node whose canonical name was
`NULL`, and `hostname` would have had nothing to read. **The prerequisite is in
this lane** — `posix/**` is lane B's — so this was not a request to file, it
was two pieces of work in order:

1. ~~implement `AI_CANONNAME` in `posix`~~ — **DONE 2026-09-14**, see below;
2. then change `hostname` to use it — **still open, and step 1 did not unblock
   it.** Read the next subsection before starting it.

### STEP 1 DONE, 2026-09-14 — and it does not fix `hostname -f`

`gai_alloc` now takes a `canon: Option<&[u8]>` and copies the name into the
same block, after the `SockaddrIn`, honouring the constraint the old comment
stated; `getaddrinfo` attaches it to the head node only, and only when
`AI_CANONNAME` is set and `nodename` is non-null. Four tests cover it,
including a control (no flag ⇒ still `NULL`) and an assertion that the
`SockaddrIn` survives — writing the name at the wrong offset inside the shared
block reads `sin_family` back as `14641`, the ASCII `"19"` of the test's
`"192.168.1.1"`, which is how that assertion was confirmed to fail when it
ought to.

**What that fixed is a NULL dereference, not the FQDN.** A program that sets
`AI_CANONNAME` and does `printf("%s", res->ai_canonname)` — which is normal,
since glibc guarantees the field is non-NULL there — was dereferencing NULL
against our libc. That is now correct, and it is worth having on its own.

**What it cannot fix, and why step 2 is still blocked.** The canonical name we
return is *the queried name itself*, because there is nothing else to return:

* `gethostbyname` fills `h_name` from its own `name` argument
  (`HostentBuf::fill(…, name, name_len, resolved)`), not from the answer;
* the answer has no room for a name — `SYS_DNS_RESOLVE` takes
  `(hostname_ptr, hostname_len, output_ptr)` and writes **four address bytes**.

So the resolver cannot report a CNAME, and `AI_CANONNAME` echoes its input: a
short hostname in, a short hostname out. That is precisely what glibc does for
a host with no CNAME, and precisely what `hostname -f` must *not* do.
**Step 2 as written would replace an invented FQDN with a short name — it
would not produce a correct one.** Whether that trade is an improvement (a
confidently wrong answer versus an admittedly incomplete one) is a real
question, and not one to settle silently while implementing it.

Closing this properly needs the FQDN to exist somewhere a libc call can reach:
either `SYS_DNS_RESOLVE` grows a canonical-name output, or the kernel's hosts
table in `kernel/src/fs/nameservice.rs` becomes readable by name. Both are
lane A's, so both are requests rather than work.

**Scale, so the next reader knows what they are picking up:**
`scripts/hostname-diff.sh` is **7 passed / 50 differed** — by ratio the worst
harness in the tree, and the largest single family of those is this one cause.
Doing (1) without (2) fixes nothing visible — and, as the subsection above
records, (1) is now done and (2) turns out to need a third thing that is not
in this lane, so the ratio is unchanged and will stay that way until the
resolver can carry a name.

Written down because the entry as it stood sends someone to `getaddrinfo` with
a plausible plan and no warning that the call cannot answer.

**And one thing that is NOT wrong, checked on the way.** Reading the above, the
next step looked alarming: nothing in `posix` reads `/etc/hosts` — the path is
defined in `paths.rs` and referenced by nothing else, and `getaddrinfo`
resolves by numeric parse, then the DNS syscall. That reads like "`localhost`
does not resolve", which would be far worse than an FQDN.

**That paragraph was right, and the "reassurance" that followed it here was
wrong. See `B-POSIX-LOCALHOST-IS-RESOLVED-BY-ASKING-A-DNS-SERVER` below.**

What this entry said until 2026-09-14 was: *"It is not the case.
`kernel/src/fs/nameservice.rs` holds the hosts table, `localhost` and
`ip6-localhost` included, and the DNS syscall `getaddrinfo` calls goes there."*
The first half is true — that table exists and has those entries. The second
half is false: `sys_dns_resolve` calls **`crate::net::dns::resolve`**, which is
a different module that never consults `fs::nameservice` at all.

**The lesson, which is the reusable part.** I went looking for something that
would explain `localhost` working, found a module named `nameservice` holding a
hosts table with `localhost` in it, and stopped — the name matched the
behaviour I was hoping to confirm. I never traced the call. Had I, the chain is
two greps long: `sys_dns_resolve` → `net::dns::resolve` → the wire.
**Finding a component that *would* explain the behaviour is not evidence that
it is the component in use** — and it is more dangerous than finding nothing,
because it ends the search with a false result instead of an open question. The
alarming reading was correct and I talked myself out of it with a plausible
mechanism, which is the worse of the two ways to be wrong here.

**Corrected 2026-09-14: the paragraph above said "there is no
`gethostbyname`". That was false** — it is `posix/src/socket.rs:3892`, and
`getaddrinfo` calls it for every non-numeric name. The grep behind the claim
was `pub extern "C" fn gethostbyname`, which misses the actual signature
`pub unsafe extern "C" fn`. The conclusion the paragraph reaches survives
(resolution is kernel-side, `localhost` does resolve), but it reached it
through a false premise, in a paragraph whose entire purpose was to stop a
misleading claim being recorded. Noted rather than quietly edited because the
false version was committed and pushed, and because the failure is a reusable
one: **an absence proved by grepping for a signature is only as good as the
modifiers in the pattern.** `unsafe`, `pub(crate)`, `async`, `const` and a
line break after `fn` all defeat it. Grep for the bare name first, then narrow.

### MOSTLY FIXED 2026-09-16 — by a route this entry ruled out without considering

`hostname -f` and `-d` now read `/etc/hosts` directly and agree with
net-tools. `scripts/hostname-diff.sh` went **13 passed / 47 differed to 21 /
39**; the eight rows that went green are this family.

**The blockage above is real and was not the only path.** Everything this
entry says about `getaddrinfo` holds: `AI_CANONNAME` returns the query echoed
back, `SYS_DNS_RESOLVE` carries four address bytes with nowhere to put a name,
and going that way would trade an invented FQDN for a short one. That reasoning
is sound and the conclusion drawn from it — "both are lane A's, so both are
requests rather than work" — did not follow, because it was reasoning about
*one mechanism*.

`hostname` is **file-based by deliberate design**, which its own module header
explains at length, and it already read five files directly:
`/proc/sys/kernel/hostname`, `/etc/hostname`, `/etc/resolv.conf`,
`/proc/net/if_inet` and `/sys/class/net`. A sixth needs no resolver, no
syscall and no other lane. nsswitch consults `files` before `dns` on every
normal system, so reading the hosts table *is* what the C library would have
done first.

**The lesson is about the shape of the blockage, not about hosts files.** This
entry blocked on "the mechanism I chose cannot answer" and then generalised to
"this cannot be answered here". Those are different statements, and the second
does not follow from the first. The question worth asking before filing a
request is not *"is my approach blocked?"* but *"is every approach blocked?"* —
and the answer here was sitting in the program's own module header.

**What remains genuinely blocked**, and is correctly described above: a host
whose FQDN lives only in DNS, with nothing in `/etc/hosts`. That still needs
the resolver to carry a canonical name, and that is still lane A's. The new
code falls back to the old search-domain behaviour there, so nothing regressed
— it is better where the hosts table answers and unchanged where it does not.

The remaining 39 rows are other causes: `-i`/`-I` address formatting, `-a` and
`-A`, and `-b`. They are not this entry.

### FIXED 2026-10-01 — by the route this entry first named

`hostname` is now a port of Debian's `hostname` 3.23
(`userspace/coreutils/src/bin/hostname`) and resolves exactly as upstream
does: `getaddrinfo` with `AI_CANONNAME` on what `gethostname` returns, by way
of `libcall::netdb`. The obstacle above is gone from both ends -- the C library
reads `/etc/hosts` first and takes the first name on the matching line as the
canonical one (lane D's `D-POSIX-HOSTS-FILE-WAS-NEVER-READ`), and the kernel's
resolver consults its hosts table -- so the hosts-file reading the 2026-09-16
section added had become a second resolver that could only disagree with the
first, and it is deleted with the rest of the file-based program.
`scripts/hostname-diff.sh`: **123 passed, 0 differed** (it was 26 / 34), now
also run under the four names the program answers to.

**What is left is the library's, not this program's:** a host whose FQDN lives
only in DNS. The kernel's resolver answers an address and no name, so the
canonical name is the name asked (`posix/src/hosts.rs`'s module doc says so),
where glibc would answer with the name the search list completed. `hostname -f`
prints whatever `getaddrinfo` says, as on Linux, and will follow the library
without a change here.
