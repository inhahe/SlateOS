## TD-A-KSHELL-PRINTS-AN-OPERAND-AS-REQUIRED-AND-THEN-SUPPLIES-IT (lane A, 2026-09-10) — **open**, 83 sites (a floor)

**In short:** many kernel-shell commands print a help line saying an argument is
required, and then, if you leave that argument out, quietly make one up and carry
on. `apppermissions grant` is the clearest case: its own help says
`grant <app> <perm>`, and running it with no arguments grants the `storage`
permission to an application literally named `app`. Nothing fails, nothing warns.
The command reports success for a decision about a subject the operator never
named.

This is the same family as the entry above, found by a better question. That one
asks "is the invented value a number?", which is a proxy. This one asks "did the
command's own help promise this operand was required?", which is the actual
question, needs no judgment, and reads the answer out of the tree.

### The convention the tree already follows

kshell documents operands the usual way: angle brackets are required, square
brackets are optional. 2348 required operand positions are documented across 269
commands, and the overwhelming majority honour it — so this is a minority of sites
breaking a convention the codebase otherwise keeps, not a missing convention.

Measured on `kernel/src/kshell.rs` at af70253de:

| | sites | meaning |
|---|---|---|
| correct | 269 | defaults to the empty string, and something tests it before use |
| tolerable | 90 | defaults to the empty string, nothing in the arm tests it — an empty string rarely names a real object, so the callee errors rather than acting on the wrong one |
| dead | 5 | a default that cannot be reached, because success requires a *later* operand and operands are positional |
| **defect** | **83** | documented required, defaulted to a usable value, nothing refuses it |

Of the 83, **78 are non-numeric** and therefore invisible to
`check-absent-operand-default.py`, which only counts numeric defaults. Only 3 of
the 83 are among that checker's 34, so the two populations are nearly disjoint.

Worked examples, each read in full rather than pattern-matched:

```
apppermissions  help: grant <app> <perm>     code: app -> "app", perm -> "storage"
contextmenu     help: build <target>         code: target -> "file"
drvmon          help: register <bus>         code: bus -> "pci"
netdiag         help: ping <host>            code: host -> "127.0.0.1"
speechio        help: listen <start|stop>    code: -> "start"
wintiling       help: create <name>          code: name -> "New"
```

`netdiag ping` with no host pinging 127.0.0.1 is harmless. `apppermissions grant`
with no app is a permission decision about an invented subject, and
`speechio listen` defaulting to `start` means a bare word turns the microphone on.
Severity varies across the 83; the shape does not.

### Why 83 is a floor, not a count

The matcher recognises `let <var> = parts.get(n)...unwrap_or("...")` and misses
defaults wrapped in a call — `parse_perm(parts.get(2).copied().unwrap_or("storage"))`
is a real site in `cmd_apppermissions` that is not among the 83. Sites whose
command prints two synopses disagreeing about whether a position is required are
skipped rather than guessed. Both omissions are deliberate: a gate that overstates
gets switched off.

Two earlier measurements of this same population were wrong and looked right, and
the corrections are the reason the table above is trusted:

* **376**, from treating the empty string as an invented value. An empty default
  followed by `if x.is_empty() { usage; return; }` is the correct idiom and the
  single most common thing these commands do — 269 sites.
* **138**, from two parser defects. `"discoverable" | "disc-mode"` did not match an
  alias pattern of `[a-z0-9_]+` — a **hyphen** — so the arm boundary was missed and
  findings were attributed to the previous subcommand. And `netshare mount` was
  reported as defaulting a path to the filesystem root when in fact success
  requires operand 3, which positionally guarantees operand 2, so that default is
  unreachable. The alarming headline was my parser's bug, not the kernel's.

### The proper fix

Per site: refuse the absent operand with the command's existing usage line, the
idiom already used 457 times in this file:

```rust
let Some(app) = parts.get(1) else {
    shell_println!("Usage: apppermissions grant <app> <perm>");
    set_exit(1);
    return;
};
```

Structurally, the criterion belongs in a gate, because it is decidable from the
tree with no judgment: *an operand the command's own help prints in angle brackets
must not have a default.* That gate should probably absorb
`check-absent-operand-default.py` rather than sit beside it — the numeric rule is a
weaker proxy for the same question, and the near-disjointness of the two
populations is an artifact of this matcher not recognising the
`match parts.get(n).unwrap_or(&"0").parse()` form rather than a real property. Two
ledgers counting one family is the double-count this lane warned lane B about the
same afternoon. Not built yet; the analysis above is the specification.

### Resolution of A-SYSFS-KEEPS-A-THIRD-HOSTNAME-THAT-NOTHING-ELSE-READS (lane A, 2026-09-10)

Fixed, and the count in the title was low. There were **four** stores for the
hostname, not three, and the fourth was the only one that accepted a write from the
operator and confirmed it.

| store | who read it | state |
|---|---|---|
| `fs::nameservice` | `/proc/sys/kernel/hostname`, `uname(2)`, `gethostname`, kshell | the one store, kept |
| posix process-local buffer | `gethostname`/`uname -n` inside one program | removed by lane B (`15c50477f`, `49b051aeb`) |
| `fs::sysfs` static | `/sys/kernel/hostname`, **and `vmguest` → the hypervisor** | removed |
| `fs::netsettings` field | a `/proc` netsettings file, and the kshell command that wrote it | removed |

Two things about the fourth are worth keeping.

**It confirmed a rename that never happened.** `netsettings::set_hostname` returned
`()`, so it could not fail, and the kshell command printed `Hostname set to myhost`
unconditionally while writing a field nothing else read. The command that read the
name back read the same dead field, so the lie was self-consistent from inside. That
is the shape lane B described for the posix buffer the same morning: a program could
set the name, read it back, get its own value, and conclude the system was renamed.

**The two copies had different lifecycles, which is why a second copy is never merely
redundant.** `init_defaults` seeded the netsettings hostname to `"mintos"` and
`clear_all` blanked it. Had the fix simply forwarded both to the one store, every
network-settings initialisation would have renamed the machine and every clear would
have un-named it. A duplicate is not just a value that can disagree; it is a value
with its own rules about when to change.

How it was found is the part worth repeating. After removing the sysfs static I swept
the kernel for anything still expecting `"mintos"` — purely to check I had not broken
another assertion — and the sweep returned `assert_eq!(hostname(), "mintos")` in a
module I had no reason to open. A defensive check for my own breakage found an
unrelated defect, which is the argument for scoping such a sweep wider than the
change that prompted it.

Three round-trip self-tests have now been replaced for the same reason in two days:
this module's, sysfs's, and lane B's `test_setdomainname_roundtrip`. Each set a value
through one door, read it back through the same door, and asserted they matched —
which is evidence about the buffer and reads exactly like evidence about the system.
All three now assert that the write ARRIVED at the single store.

One bound, too: `crate::uname::NODENAME_MAX` (64) replaces `fs::nameservice`'s 253 and
`SYS_HOSTNAME_SET`'s 64, which disagreed — a 100-byte name was settable through one
door, refused at another, and unreportable by `gethostname` once stored.
