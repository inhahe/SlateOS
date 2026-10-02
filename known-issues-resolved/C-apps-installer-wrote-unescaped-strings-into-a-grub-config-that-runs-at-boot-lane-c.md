## `apps/installer` wrote unescaped strings into a GRUB config that runs at boot (lane C) — FIXED

`grub.rs`'s `generate_entry` interpolated every field of a `GrubEntry` —
`title`, `kernel_path`, `root_partition`, `uuid`, `initrd_path` and each of
`kernel_params` — straight into a `menuentry` block with no quoting and no
validation:

```rust
out.push_str(&format!("menuentry \"{}\" {{\n", entry.title));
...
out.push_str(&format!("    chainloader {}\n", entry.kernel_path));
```

That block is written to `/etc/grub.d/40_slateos` (mode 0755) and folded into
`grub.cfg` by `update-grub`. **GRUB executes `grub.cfg` at boot with full
firmware privilege — before any OS, and therefore before any OS-level security
boundary exists.** A title containing a `"` closes the string and everything
after it is parsed as fresh GRUB script; a title containing a newline does not
even need the quote. `$` expanded as a GRUB variable.

The reachability is the part worth remembering: this looked like a field the
user types into our own installer, so "who would attack themselves?". But
`os-prober` — the whole reason this module exists — *scrapes* menu titles out
of **other partitions'** `/etc/os-release`. On a dual-boot machine that is a
file the other OS controls, so the title is attacker-influenced input arriving
through a path that never looks like input.

**Fixed** by emitting every interpolated value inside `"…"` through a new
`grub_quote`, which escapes exactly the three bytes GRUB's lexer treats
specially inside a double-quoted string — `\`, `"`, `$` — mirroring
`grub_quote()` in GRUB's own `util/grub-mkconfig_lib.in`. Control characters
cannot be escaped that way, so `GrubEntry::validate` rejects them and
`generate_entry`/`generate_custom_script` now return
`Result<String, GrubError>`; `install`/`update` validate *before* touching the
filesystem, so a rejected entry leaves no file behind.

A second, non-security bug fell out of the same rewrite: `kernel_params` were
`join(" ")`ed into the line, so a parameter containing a space silently became
two parameters. Each is now quoted individually.

**Lesson, and it generalises past this file: "config file" is not a safe
output format.** The lossy-path sweep that led here trained the question *is
this value preserved byte-for-byte?* — but preservation is only half of it.
The other half is *can this value change the meaning of the document it is
written into?* A path can round-trip perfectly and still be an injection. Any
place we `format!` a value into a file that something else later *parses* —
GRUB config, shell script, YAML, JSON, a desktop entry — needs an escaping
function chosen for that grammar, not just faithful bytes. Worth auditing the
other generators in `apps/` on the same question.

Five separate defences, verified non-vacuous by breaking each one alone and
confirming it failed only its own test: escaping `$`, escaping `"`, escaping
`\`, the control-character rejection, and the per-parameter quoting.
