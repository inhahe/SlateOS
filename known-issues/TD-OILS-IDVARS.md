### TD-OILS-IDVARS. `osh` does not define several bash identity/runtime variables (`PPID` remaining; `EUID`/`UID`/`HOSTNAME`/`BASH`/`BASHOPTS` now done) — PARTIALLY ADDRESSED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`Shell::seed_shell_vars`, the
`param_value` dynamic-var match arm around the `BASHPID`/`BASH_SUBSHELL` cases).

**Status (2026-07-19):** the static *platform-identity* trio bash always
defines is now seeded — `HOSTTYPE=x86_64`, `OSTYPE=slateos`,
`MACHTYPE=x86_64-slateos` (ordinary reassignable shell vars, SlateOS values not
the host build's). `BASHPID` and `BASH_SUBSHELL` were already dynamic.
**`BASH` and `BASHOPTS` are now defined (2026-07-19):** `BASH` is seeded from
`std::env::current_exe()` (lossy, fallback `"osh"`) as a reassignable var;
`BASHOPTS` is a readonly, colon-joined, alphabetically-sorted list of enabled
`shopt` options kept current by `refresh_bashopts()` on every `shopt` toggle
(osh now models bash's full 57-option `shopt` inventory with correct
non-interactive defaults, so the seeded set matches bash byte-for-byte —
verified against MSYS bash). Still **missing** relative to bash:

- **`EUID` / `UID`** — **RESOLVED 2026-07-21 (Q28, operator).** Both are now
  seeded in `seed_shell_vars` as real readonly-integer shell vars (`declare -ir`,
  matching bash's own attributes), defaulting to **root (`0`/`0`)** per the
  operator's Q28 decision. The reported identity is per-user configurable via the
  `OSH_UID` / `OSH_EUID` env toggles (resolved once in the free fn
  `reported_identity`), so a login/session layer can inject a real user identity.
  Seeded *before* `import_environment` (whose `or_insert` can't override and
  whose export-marking therefore never touches them), matching bash: an inherited
  `UID=` in the environment neither wins nor becomes exported. The `\$` prompt
  escape now keys `#`-vs-`$` on `$EUID == 0` instead of guessing from the name.
  Unlike the (still dynamic) `PPID`/`BASHPID` specials, these are *real* vars, so
  readonly reassignment is genuinely rejected and they correctly appear in
  `declare -i`/`declare -p`/`set`/`${!prefix*}` listings exactly as bash lists
  them. **Note / latent inconsistency:** `PPID`/`BASHPID` remain dynamic-only
  (`param_value` cases, *not* in `self.readonly`), so `PPID=5` is silently
  accepted-and-shadowed rather than rejected, and they're absent from bulk
  `declare -i`/`declare -p` listings — a pre-existing divergence from bash now
  made more visible by the more-faithful real-var model used for EUID/UID. The
  proper long-term fix is to migrate the remaining readonly-integer specials
  (`PPID`, and `BASHPID` for readonly-ness) to the same real-var model.
- **`PPID`** (parent process id, readonly in bash). ~~Needs a parent-pid source;
  `std::process` doesn't expose it portably on the host and the SlateOS syscall
  isn't wired. Deferred until a `getppid`-equivalent exists.~~ **Superseded:** the
  *source* exists — `parent_pid()` (`interp.rs`) calls `NtQueryInformationProcess`
  on Windows and libc `getppid` on unix, and `$PPID` reads correctly on both. What
  remains is only the *model*: `PPID` is a dynamic `param_value` case rather than a
  real readonly var, which is the "latent inconsistency" described in the `EUID`
  bullet above, not a missing-value problem.
- **`HOSTNAME`** — **RESOLVED 2026-07-20.** Now synthesized inside
  `import_environment` (after the `SHLVL` block) via the free function
  `system_hostname()`: on unix it reads `/proc/sys/kernel/hostname` then
  `/etc/hostname` (trimmed, first non-empty); on the host Windows build it reads
  `%COMPUTERNAME%`. Uses `contains_key` guard so an inherited `HOSTNAME` from the
  environment wins over synthesis, and a preset shell var wins over both (env
  import uses `or_insert`). On the SlateOS target neither procfs path exists yet,
  so `system_hostname()` returns `None` and `HOSTNAME` stays unset — a graceful
  fallback that never lies. This was the low-stakes naming half of the Q28 open
  question; per the operator's own framing ("`$HOSTNAME`'s default … is a
  low-stakes naming choice that can ride along") it was implemented as a
  defensible, overridable default. The genuine operator decision (root vs.
  non-root identity for `$EUID`/`$UID`) remains open and untouched.

**Proper fix:** once SlateOS credential/`getuid`/`getppid` syscalls exist, wire
`EUID`/`UID`/`PPID` as dynamic `param_value` cases (readonly). The identity
*default* (for host runs and pre-login target state) needs the operator's call.

**Update 2026-08-25 — the *host* half of that is done (commits `60a63c49f`,
`c10a69c24`).** On unix, `reported_identity` no longer consults a policy at all:
it asks libc, via `unsafe extern "C" { fn getuid() -> u32; fn geteuid() -> u32; }`,
and a sibling `os_egid()` declares `getegid` for `-G`. The shell now reports the
process's real credential on any unix-family target.

This was not cosmetic: with the identity pinned at root, `[ -O file ]` answered
*backwards* on a host — every file the user owned tested false and every
root-owned file true — and three corpus cases were failing on it.

**Why `extern "C"` and not `/proc/self/status`.** The first cut of this fix
(`60a63c49f`) read procfs, on the reasoning that `std` exposes no `getuid` and
`oils` deliberately carries no `libc` dependency, with `system_hostname`'s
`/proc/sys/kernel/hostname` read as precedent. That was rewritten in `c10a69c24`
for two reasons, both discovered by checking rather than recalling:

1. **The file already had the precedent, and it was the other one.**
   `parent_pid()` declares `unsafe extern "C" { fn getppid() -> i32; }` for unix.
   Two different mechanisms for "ask the OS a credential question," in one file,
   is a bug waiting for whoever adds the third.
2. **The failure modes are not comparable.** SlateOS mounts no procfs, so on the
   actual target the `/proc` read *fails*, and the caller falls back to the Q28
   default — a shell that answers "yes, you own it" about every file on the
   machine. A missing symbol is a link error, which is loud and immediate; a
   missing `/proc` is a silent lie about privilege. `system_hostname` is not a
   counter-example: its fallback is to leave `$HOSTNAME` unset, which lies about
   nothing.

The symbols resolve on SlateOS: `posix/src/unistd.rs` exports `getuid` (620),
`geteuid` (628), `getgid` (638) and `getegid` (646), each
`#[cfg_attr(target_os = "none", unsafe(no_mangle))]`, reading the credentials the
kernel recorded at spawn. So the Q28 decision survives on the target *by asking
the system* rather than by assuming — which is what the decision was always
standing in for. (`toolchain/x86_64-slateos.json` sets `"os": "linux"`, so
`#[cfg(unix)]` covers the target as well as the host.)

The Q28 fallback survives as the pure function `configured_identity(uid, euid)`,
still honouring `OSH_UID`/`OSH_EUID`, still defaulting to root, and reachable now
only on non-unix (the Windows host build). It takes its inputs as arguments
rather than reading the environment so that it stays testable on unix — where it
is never the answer — and so that its test does not write process-global state
that no parallel test can safely share.

**`-G` asks the effective gid and nothing else.** Measured against bash 5.2.21,
not recalled: a file `chgrp`ed to a group the caller genuinely belongs to (group
24, for a member of 24) is `[ -G f ]` **false**. The supplementary group list is
not consulted, so the earlier implementation — which derived a gid from the passwd
entry — was answering a different question. `reported_groups()` (which backs
`${GROUPS[@]}`) is unaffected and was verified against `id -G` separately.

**Sub-issue — several `BASH_*` internal variables are still absent.** `${!BASH*}`
diverges from bash because osh does not define `BASH_LOADABLES_PATH`.
(`BASH_ALIASES`/`BASH_CMDS` are now defined — **RESOLVED 2026-07-20**, see plan
turned-implementation note below.)
(`BASH_EXECUTION_STRING` — the `-c` command string — is now defined, seeded via
`Shell::set_execution_string` from `main.rs`'s `-c` path, so it reads correctly
and appears in `${!BASH*}`. `BASH_ALIASES`/`BASH_CMDS` are now defined too —
**RESOLVED 2026-07-20**, see the implementation note below. `BASH_ARGV0` is now
defined too — **RESOLVED
2026-07-20**: a dynamic variable tied to `self.name`; `BASH_ARGV0=name` sets
`$0`, reading `$BASH_ARGV0` returns the current `$0`, it appears in `${!BASH*}`
and `declare -p`. One deliberate divergence: bash's `+=` on `BASH_ARGV0` relies
on an obscure lazy-materialization quirk — `BASH_ARGV0=a; BASH_ARGV0+=b` yields
`b`, not `ab`, unless a read intervened — so osh uses the predictable append
(`ab`) instead; `BASH_ARGV0+=` is vanishingly rare in real scripts.
`BASH_ARGC`/`BASH_ARGV` — the extdebug call-argument stack — are now defined too,
**RESOLVED 2026-07-20**; see TD-OILS-MISSING-SPECIAL-ARRAYS for the full
semantics and the one documented non-extdebug divergence.) The only remaining
missing `BASH_*` var is `BASH_LOADABLES_PATH`, which is meaningless on SlateOS
(no loadable builtins) and is intentionally omitted. Low priority.

**Implementation of `BASH_ALIASES`/`BASH_CMDS` — DONE 2026-07-20.** bash exposes
these as *dynamic associative arrays* that are (a) present even when empty
(`declare -A BASH_ALIASES=()`), (b) live — reflecting the current alias table /
command hash, and (c) writable: `BASH_ALIASES[x]="…"` creates an alias,
`BASH_CMDS[foo]=/p` adds a hash entry. The chosen design is **eager mirror
sync at every source mutation** rather than the materialise-on-read accessor the
earlier plan sketched: `self.aliases`/`self.cmd_hash` remain the source of truth,
and `sync_bash_aliases`/`sync_bash_cmds` rebuild the `self.assoc["BASH_ALIASES"]`
/`["BASH_CMDS"]` mirror. The concern that eager-sync would "go stale" was
unfounded once the *complete* mutation set was enumerated — it is small and
closed: the `alias`/`unalias`/`hash` builtins plus the single `resolve_external`
insert (interp.rs ~5609, on the *new*-command branch only; the cache-hit branch
mutates just the hit count, which the mirror doesn't expose, so it stays
sync-free on the hot path). Element writes are intercepted in `assoc_set`
(interp.rs ~3630): `BASH_ALIASES[k]=v`→`self.aliases.insert`, `BASH_CMDS[k]=v`→
`self.cmd_hash.insert`, with `+=` append honoured, then a re-sync. Both names are
seeded empty in `seed_shell_vars` (present-when-empty) and marked `array_valued`
so `declare -p`/`declare -A` render `=()`. Because they live in `self.assoc`, all
existing assoc read paths (`${x[k]}`, `${x[@]}`, `${!x[@]}`, `${#x[@]}`,
`declare -p`, `${!BASH*}` enumeration, bare `declare -A` listing) work unchanged
— no read-site refactor was needed. Verified against bash: `declare -p`,
element read, count, `+=`, `unalias -a`, `hash -p`, and `${!BASH_ALIASES@}`
enumeration all match; regression test `bash_aliases_and_cmds_live_assoc`.
*One documented cosmetic divergence:* `${!BASH_ALIASES[@]}` key order is
`self.aliases` (BTreeMap) sorted order, whereas bash uses its internal hash order
(e.g. bash `b a` vs osh `a b` for aliases `a`,`b`) — unspecified/arbitrary in
bash, and osh's sorted order is deterministic and matches osh's own `alias`
builtin listing.

**Sub-issue — dynamic vars are readable but not *enumerated*.** The dynamic
`param_value` cases (`BASHPID`, `BASH_SUBSHELL`, and any future `EUID`/…)
return a value when read directly (`echo $BASHPID`) but are **not listed** by the
name-prefix expansions `${!BASH*}` / `${!BASH@}`, because those enumerate only the
concrete `vars`/`arrays`/`assoc` maps. (`BASH`/`BASHOPTS` are concrete `vars` now,
so they *do* appear.) bash includes every dynamic variable in the prefix listing.
Fixing this needs the prefix-match code to also consider the set of known
dynamic-variable names (a static name list checked alongside the maps).
Low-value (prefix enumeration of `BASH*` is rare in scripts) and coupled to the
broader "define the missing `BASH*` vars" work above, so parked here.
