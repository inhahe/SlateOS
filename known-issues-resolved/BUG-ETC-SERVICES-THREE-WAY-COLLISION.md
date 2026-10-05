### BUG-ETC-SERVICES-THREE-WAY-COLLISION. Three subsystems claimed `/etc/services`, two of them as a file and one as a directory — 2026-07-31 — ✅ **RESOLVED 2026-07-31**

**What.** Three independent consumers had each picked the path
`/etc/services`, for three incompatible things:

1. **The IANA port/protocol database** (a plain file, the classic Unix
   meaning) — `userspace/getent/src/main.rs:25`
   (`const SERVICES_FILE: &str = "/etc/services";`, read at line 270 with
   `fs::read_to_string`), and `posix/src/paths.rs:60`
   (`pub const _PATH_SERVICES: &[u8] = b"/etc/services\0";`).
2. **A flat list of startup services** (also a plain file, but a
   line-per-ELF-path format of our own) — `services/init/src/main.rs`
   `load_startup_services` (`fs_read_file(b"/etc/services", …)`), plus
   `kernel/src/main.rs`, which *creates* the file during boot bring-up with
   `# Startup services (one per line)` in it.
3. **A directory of YAML unit definitions** — `userspace/service/src/main.rs`
   (`/etc/services/<name>.service`, `fs::read_dir("/etc/services")`, and
   enable-symlinks in `/etc/services/enabled/<name>`).

A single path cannot be a plain file with two different formats *and* a
directory, so on a real boot at least two of the three were guaranteed to be
broken. In practice the kernel wins the race — it writes the file before
init runs — so `service list`'s `read_dir` fails (silently: it is inside
`if let Ok(entries)`, so the CLI just reports no defined services), `service
enable` cannot create its symlink, and `getent services` parses init's
startup list as though it were port assignments and prints garbage or
nothing.

**How it was found.** Surveying `compgen -A service` for osh, which would
read `/etc/services`. Grepping for the backing file turned up all three
readings at once.

**Fix (applied).** `/etc/services` keeps the standard meaning and the other
two move; see design-decisions.md §96 for why that direction and not
another.

- The IANA database keeps `/etc/services`. `userspace/getent` and
  `posix/src/paths.rs` are unchanged.
- Init's startup list is now **`/etc/startup.conf`** —
  `services/init/src/main.rs` (all nine references, including the `svc cfg`
  help line and the reload path), `services/ticker/src/main.rs`'s module
  doc, and the kernel's bootstrap write in `kernel/src/main.rs`.
- The service manager's unit directory is now **`/etc/service.d/`** —
  `userspace/service/src/main.rs` (definitions at
  `/etc/service.d/<name>.service`, enable-symlinks at
  `/etc/service.d/enabled/<name>`). Runtime state stays at
  `/run/services/<name>/`, which never collided with anything.

Each of the three sites carries a comment saying why the name is what it is,
so the collision does not get re-introduced by someone reaching for the
"obvious" path.

**Verified.** `cargo build --release` clean for `services/init` and
`services/ticker` (bare-metal `x86_64-unknown-none`), `cargo check -p kernel`
clean, and `cargo clippy -p service` clean on `x86_64-slateos` (which also
took out a pre-existing `redundant reference in print! argument` warning at
`userspace/service/src/main.rs:668`). No docs, scripts or tests referenced
the old paths — only archived QEMU serial logs under `build/hang-catches/`,
which are historical records and were left alone.
