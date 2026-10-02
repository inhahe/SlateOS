### TD-OILS-PROMPT-HOSTNAME-IS-THE-WINDOWS-SPELLING. `\h` shouts where bash does not — 2026-08-04 — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the prompt decoder's `\h`/`\H`
escapes read `COMPUTERNAME`, which Windows spells in upper case. MSYS bash reads
the DNS host name and gets the mixed-case one.

**Reproduce:**

```sh
x='h:\h w:\$'
echo "${x@P}"
# bash: h:Logoplex3 w:$
# osh:  h:LOGOPLEX3 w:#
```

The `\$` half of that line is **not** a bug: `#` is right because osh reports
`EUID=0` on purpose (open-questions Q28 option A, root for the pre-privilege
bring-up — see `reported_identity`), and MSYS bash is the one synthesising a
non-zero UID. Only the hostname's case differs for no reason.

**The fix.** `Shell::system_hostname`'s Windows arm now calls
`GetComputerNameExW(ComputerNameDnsHostname, …)` instead of reading
`COMPUTERNAME`, which is the upper-cased *NetBIOS* spelling. The buffer starts at
64 wide chars and grows once on `ERROR_MORE_DATA`, since the call reports the
size it wants; `COMPUTERNAME` remains as a fallback, an upper-cased name being
better than none. The Unix arm (procfs, then `HOSTNAME`) is untouched. On SlateOS
this becomes whatever the real host-name call returns and the question
disappears.

**Pinned by** `the_windows_host_name_is_not_the_netbios_spelling` in `interp.rs`,
which asserts the two spellings differ only in case — the machine name itself
cannot be hard-coded into a test. No corpus case can pin it either: a case that
printed the host name would only match on the machine that wrote it, which is
why this went unnoticed in the first place.
