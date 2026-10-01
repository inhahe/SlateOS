//! The installer's GRUB commands: find a GRUB on this machine, and add,
//! change or remove the menu entry that starts Slate OS from it.
//!
//! GRUB cannot load Slate OS's kernel itself -- the kernel has no multiboot2
//! header -- so the entry chainloads Limine from an EFI system partition,
//! found by that partition's filesystem UUID ([`installer::grub_entry`]).
//!
//! The entry is a script in the system's `/etc/grub.d/`, never an edit to its
//! `grub.cfg`: that system rebuilds `grub.cfg` from its scripts and would
//! write over an edit. For the running system (`--root /`, the default) the
//! rebuild is done here, with its own `update-grub` or `grub-mkconfig`, and
//! the rebuilt menu is read back to see that the change is in it. For another
//! system's root, mounted somewhere, the rebuild is left to that system: this
//! machine's tools rebuild this machine's menu, not that one's.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use installer::grub::{
    self, EntryState, GrubDetector, GrubEntry, GrubError, GrubInstall, GrubInstaller,
    GrubUpdateRunner, GrubVersion,
};
use installer::{GRUB_TITLE, LIMINE_EFI_PATH, grub_entry};
use pathtext::ShowPath;

/// Why `--direct` is refused, for anyone who finds the strategy in `grub.rs`.
const DIRECT_REFUSED: &str = "GRUB cannot start Slate OS's kernel itself: the kernel has no \
    multiboot2 header for GRUB's multiboot2 command to load it by. The entry chainloads \
    Limine, which does start it -- leave out --direct.";

/// The UEFI variable that says whether Secure Boot is on, under a root.
const SECURE_BOOT_VAR: &str =
    "sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// A GRUB command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Report the GRUB found, and where Slate OS's entry stands in it.
    Detect,
    /// Add this entry.
    Add(GrubEntry),
    /// Rewrite the entry that is there as this one.
    Update(GrubEntry),
    /// Remove the entry.
    Remove,
}

/// Which command a flag asks for, before its options are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// `--grub-detect`.
    Detect,
    /// `--grub-add`.
    Add,
    /// `--grub-update`.
    Update,
    /// `--grub-remove`.
    Remove,
}

impl Verb {
    fn flag(self) -> &'static str {
        match self {
            Self::Detect => "--grub-detect",
            Self::Add => "--grub-add",
            Self::Update => "--grub-update",
            Self::Remove => "--grub-remove",
        }
    }

    /// Whether the command writes an entry, and so is told one.
    fn writes_entry(self) -> bool {
        matches!(self, Self::Add | Self::Update)
    }
}

/// A GRUB command and everything it was told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// What to do.
    pub action: Action,
    /// The root of the system whose GRUB it is: `/` for the running one.
    pub root: PathBuf,
    /// Whether to rebuild the running system's menu after a change
    /// (`--no-update` says not to).
    pub rebuild: bool,
}

/// Read a GRUB command's options: the arguments after its flag.
pub fn parse(verb: Verb, args: &[OsString]) -> Result<Command, String> {
    let mut root: Option<PathBuf> = None;
    let mut uuid: Option<String> = None;
    let mut path: Option<String> = None;
    let mut title: Option<String> = None;
    let mut no_update = false;

    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--root") => set(
                &mut root,
                "--root",
                PathBuf::from(value(&mut args, "--root")?),
            )?,
            Some("--uuid") => set(
                &mut uuid,
                "--uuid",
                text(value(&mut args, "--uuid")?, "--uuid")?,
            )?,
            Some("--path") => set(
                &mut path,
                "--path",
                text(value(&mut args, "--path")?, "--path")?,
            )?,
            Some("--title") => {
                set(
                    &mut title,
                    "--title",
                    text(value(&mut args, "--title")?, "--title")?,
                )?;
            }
            Some("--no-update") => {
                if no_update {
                    return Err("--no-update given twice".to_string());
                }
                no_update = true;
            }
            Some("--direct") => return Err(DIRECT_REFUSED.to_string()),
            _ => {
                return Err(format!(
                    "unknown option '{}' for {}",
                    arg.as_os_str().shown(),
                    verb.flag()
                ));
            }
        }
    }

    // An option a command does not use is refused rather than ignored: a
    // user who gave it expected it to do something.
    if !verb.writes_entry() {
        for (given, option) in [
            (uuid.is_some(), "--uuid"),
            (path.is_some(), "--path"),
            (title.is_some(), "--title"),
        ] {
            if given {
                return Err(format!("{option} means nothing to {}", verb.flag()));
            }
        }
    }
    if no_update && verb == Verb::Detect {
        return Err("--no-update means nothing to --grub-detect".to_string());
    }

    let action = match verb {
        Verb::Detect => Action::Detect,
        Verb::Remove => Action::Remove,
        Verb::Add | Verb::Update => {
            let entry = entry_from(verb, uuid, path, title)?;
            if verb == Verb::Add {
                Action::Add(entry)
            } else {
                Action::Update(entry)
            }
        }
    };
    Ok(Command {
        action,
        root: root.unwrap_or_else(|| PathBuf::from("/")),
        rebuild: !no_update,
    })
}

/// The entry `--grub-add` or `--grub-update` writes, from its options.
fn entry_from(
    verb: Verb,
    uuid: Option<String>,
    path: Option<String>,
    title: Option<String>,
) -> Result<GrubEntry, String> {
    let uuid = uuid.ok_or_else(|| {
        format!(
            "{} needs --uuid: the filesystem UUID of the EFI system partition Limine is on \
             (blkid shows it)",
            verb.flag()
        )
    })?;
    if !grub::is_valid_uuid(&uuid) {
        return Err(format!(
            "'{uuid}' is not a filesystem UUID -- an EFI system partition's looks like 1A2B-3C4D"
        ));
    }
    let title = title.unwrap_or_else(|| GRUB_TITLE.to_string());
    if title.trim().is_empty() {
        return Err("--title is empty: GRUB's menu would show a blank line".to_string());
    }
    let path = path.unwrap_or_else(|| LIMINE_EFI_PATH.to_string());
    if !path.starts_with('/') {
        return Err(format!(
            "--path '{path}' must start at the partition's root, with /"
        ));
    }
    let mut entry = grub_entry(&title, &uuid);
    entry.kernel_path = path;
    // A control character cannot be quoted into GRUB's script; say so now
    // rather than after the command has found GRUB and started writing.
    entry.validate().map_err(|e| e.to_string())?;
    Ok(entry)
}

/// The value after `option`.
fn value<'a>(
    args: &mut std::slice::Iter<'a, OsString>,
    option: &str,
) -> Result<&'a OsString, String> {
    args.next().ok_or_else(|| format!("{option} needs a value"))
}

/// A value that has to be text: it is written into GRUB's script.
fn text(value: &OsStr, option: &str) -> Result<String, String> {
    value
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{option} '{}' is not text", value.shown()))
}

/// Keep an option's value, refusing a second one.
fn set<T>(slot: &mut Option<T>, option: &str, value: T) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{option} given twice"));
    }
    *slot = Some(value);
    Ok(())
}

/// The running system, as far as a command reaches past the files under its
/// root. The tests stand in for it, so none of them looks for or runs the
/// machine's own GRUB tools.
struct Host<'a> {
    /// Whether the command's root is the running system's.
    live: bool,
    /// The running system's tool for rebuilding GRUB's menu, if it has one.
    tool: &'a dyn Fn() -> Option<&'static str>,
    /// Rebuild the running system's menu, returning the tool that did.
    rebuild: &'a dyn Fn(&GrubInstall) -> Result<&'static str, GrubError>,
}

/// Carry out a GRUB command, returning what to tell the user.
pub fn run(cmd: &Command) -> Result<String, String> {
    let rebuild = |install: &GrubInstall| {
        GrubUpdateRunner::with_output_path(&install.config_path).update_grub()
    };
    let host = Host {
        live: cmd.root == Path::new("/"),
        tool: &GrubUpdateRunner::detect_command,
        rebuild: &rebuild,
    };
    run_on(cmd, &host)
}

fn run_on(cmd: &Command, host: &Host<'_>) -> Result<String, String> {
    let detector = GrubDetector::with_root(&cmd.root);
    let install = detector.detect().ok_or_else(|| {
        format!(
            "no GRUB found under {}: it has no grub.cfg in any of the places GRUB keeps one",
            cmd.root.shown()
        )
    })?;
    let scripts = detector.detect_custom_dir();
    match &cmd.action {
        Action::Detect => Ok(describe(cmd, host, &install, scripts.as_deref())),
        Action::Add(_) | Action::Update(_) | Action::Remove => {
            change(cmd, host, &install, scripts.as_deref())
        }
    }
}

/// What `--grub-detect` says.
fn describe(
    cmd: &Command,
    host: &Host<'_>,
    install: &GrubInstall,
    scripts: Option<&Path>,
) -> String {
    let version = match install.version {
        GrubVersion::Grub2 => "GRUB 2",
        GrubVersion::Legacy => "GRUB Legacy",
    };
    let mut lines = vec![format!("{version}: {}", install.config_path.shown())];
    let mut row = |label: &str, value: String| lines.push(format!("  {label:<15} {value}"));

    row("Firmware:", firmware(install, host.live).to_string());
    match secure_boot(&cmd.root) {
        Some(true) => row(
            "Secure Boot:",
            "on -- GRUB will not start Limine until it is enrolled".to_string(),
        ),
        Some(false) => row("Secure Boot:", "off".to_string()),
        None => {}
    }
    let in_menu = in_menu(install);
    match scripts {
        Some(dir) => {
            row("Menu scripts:", dir.shown().to_string());
            let installer = GrubInstaller::new(dir);
            let script = dir.join(grub::CUSTOM_SCRIPT_NAME);
            let state = match installer.state() {
                Ok(EntryState::Ours) if in_menu => "in the menu".to_string(),
                Ok(EntryState::Ours) => "written; in the menu once it is rebuilt".to_string(),
                Ok(EntryState::Absent) if in_menu => {
                    "removed; in the menu until it is rebuilt".to_string()
                }
                Ok(EntryState::Absent) => "none".to_string(),
                Ok(EntryState::Foreign) => {
                    format!("none -- {} is someone else's", script.shown())
                }
                Err(e) => format!("{} could not be read: {e}", script.shown()),
            };
            row("Slate OS entry:", state);
        }
        None => {
            row(
                "Menu scripts:",
                "none -- this system writes its menu some other way".to_string(),
            );
            row("Slate OS entry:", "none".to_string());
        }
    }
    if let Some(config) = grub::parse_grub_config(&cmd.root) {
        row("Timeout:", format!("{} s", config.timeout));
        row("Default entry:", config.default_entry);
        row(
            "os-prober:",
            if config.os_prober_enabled {
                "on"
            } else {
                "off"
            }
            .to_string(),
        );
    }
    if host.live {
        row(
            "Rebuilt by:",
            (host.tool)().map_or_else(
                || "nothing found (update-grub, grub2-mkconfig, grub-mkconfig)".to_string(),
                str::to_string,
            ),
        );
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// How the system under the root was started, as far as can be told.
///
/// `/sys/firmware/efi` exists only on a running system started through UEFI,
/// so its absence says "BIOS" only about the running system; under another
/// system's root, mounted without its `/sys`, it says nothing.
fn firmware(install: &GrubInstall, live: bool) -> &'static str {
    if install.is_efi() {
        "UEFI"
    } else if live {
        "BIOS -- GRUB started this way cannot chainload Limine"
    } else {
        "not known -- not the running system"
    }
}

/// Whether Secure Boot is on, when the root's firmware variables say.
///
/// The variable is four bytes of attributes and one of value.
fn secure_boot(root: &Path) -> Option<bool> {
    let bytes = fs::read(root.join(SECURE_BOOT_VAR)).ok()?;
    bytes.get(4).map(|&on| on == 1)
}

/// Whether GRUB's menu, as last rebuilt, has the Slate OS entry in it:
/// `grub-mkconfig` fences each script's output with the script's path.
fn in_menu(install: &GrubInstall) -> bool {
    let fence = format!("### BEGIN /etc/grub.d/{} ###", grub::CUSTOM_SCRIPT_NAME);
    let fence = fence.as_bytes();
    fs::read(&install.config_path).is_ok_and(|cfg| cfg.windows(fence.len()).any(|w| w == fence))
}

/// `--grub-add`, `--grub-update` and `--grub-remove`.
fn change(
    cmd: &Command,
    host: &Host<'_>,
    install: &GrubInstall,
    scripts: Option<&Path>,
) -> Result<String, String> {
    let entry = match &cmd.action {
        Action::Add(entry) | Action::Update(entry) => Some(entry),
        Action::Detect | Action::Remove => None,
    };
    if entry.is_some() && host.live && !install.is_efi() {
        return Err(
            "this machine was started through the BIOS, and GRUB started that way cannot \
             chainload Limine, which is an EFI program: start the machine through UEFI to add \
             Slate OS to its menu"
                .to_string(),
        );
    }
    let Some(dir) = scripts else {
        return Err(match entry {
            Some(entry) => {
                let block = grub::generate_entry(entry).map_err(|e| e.to_string())?;
                format!(
                    "GRUB under {} has no etc/grub.d for menu scripts, so this system writes its \
                     menu some other way (NixOS does, from configuration.nix). The entry to add \
                     there:\n\n{block}",
                    cmd.root.shown()
                )
            }
            None => format!(
                "GRUB under {} has no etc/grub.d, so it has no Slate OS script to remove",
                cmd.root.shown()
            ),
        });
    };

    let installer = GrubInstaller::new(dir);
    let script = dir.join(grub::CUSTOM_SCRIPT_NAME);
    let result = match &cmd.action {
        Action::Add(entry) => installer.install(entry),
        Action::Update(entry) => installer.update(entry),
        Action::Remove => installer.uninstall(),
        Action::Detect => Ok(()),
    };
    result.map_err(|e| explain(&e, &cmd.action, dir))?;

    let mut report = match entry {
        Some(entry) => format!(
            "{} the Slate OS entry, '{}', chainloading {} from the partition {}: {}\n",
            if matches!(cmd.action, Action::Update(_)) {
                "Rewrote"
            } else {
                "Wrote"
            },
            entry.title,
            entry.kernel_path,
            entry.uuid,
            script.shown()
        ),
        None => format!("Removed the Slate OS entry: {}\n", script.shown()),
    };
    if entry.is_some() && secure_boot(&cmd.root) == Some(true) {
        report.push_str(
            "Secure Boot is on: GRUB will not start Limine, which no key the firmware trusts \
             has signed, until Limine is enrolled with shim's MokManager (\"Enroll hash from \
             disk\") or Secure Boot is turned off in the firmware's settings.\n",
        );
    }

    if !(host.live && cmd.rebuild) {
        report.push_str(&rebuild_by_hand(cmd, host.live, install));
        return Ok(report);
    }
    let tool = match (host.rebuild)(install) {
        Ok(tool) => tool,
        Err(GrubError::GrubNotFound) => {
            return Err(format!(
                "{report}No update-grub or grub-mkconfig was found to rebuild GRUB's menu, and \
                 the change is not in the menu until it is rebuilt."
            ));
        }
        Err(e) => return Err(format!("{report}Rebuilding GRUB's menu failed: {e}")),
    };
    // Read the menu back: a script GRUB skips -- one that is not executable,
    // say -- rebuilds cleanly and leaves the entry out.
    let wanted = entry.is_some();
    if in_menu(install) != wanted {
        return Err(format!(
            "{report}{tool} rebuilt GRUB's menu, but {}",
            if wanted {
                "the entry is not in it: is the script executable?"
            } else {
                "the entry is still in it"
            }
        ));
    }
    report.push_str(&format!(
        "{tool} rebuilt GRUB's menu; the entry is {} it.\n",
        if wanted { "in" } else { "gone from" }
    ));
    Ok(report)
}

/// How the change reaches the menu when it was not rebuilt here.
fn rebuild_by_hand(cmd: &Command, live: bool, install: &GrubInstall) -> String {
    // The menu's path as that system sees it: under its own root.
    let config = install.config_path.strip_prefix(&cmd.root).map_or_else(
        |_| install.config_path.clone(),
        |rel| Path::new("/").join(rel),
    );
    let config = config.shown().to_string();
    let mkconfig = if config.contains("grub2") {
        "grub2-mkconfig"
    } else {
        "grub-mkconfig"
    };
    let how = format!("update-grub (or {mkconfig} -o {config})");
    if live {
        format!("GRUB's menu has the change once it is rebuilt: run {how}.\n")
    } else {
        format!(
            "That system's menu has the change once it rebuilds it: start it and run {how} \
             there. This machine's tools would rebuild this machine's menu instead.\n"
        )
    }
}

/// A GRUB error, said in terms of what to do about it.
fn explain(e: &GrubError, action: &Action, dir: &Path) -> String {
    match e {
        GrubError::EntryAlreadyExists => format!(
            "Slate OS already has an entry in {}; --grub-update rewrites it",
            dir.shown()
        ),
        GrubError::EntryNotFound => format!(
            "there is no Slate OS entry in {} to {}; --grub-add adds one",
            dir.shown(),
            if matches!(action, Action::Remove) {
                "remove"
            } else {
                "rewrite"
            }
        ),
        GrubError::NotOurs(_) => format!("{e} -- move it aside for the installer to write its own"),
        GrubError::Io(io) if io.kind() == io::ErrorKind::PermissionDenied => {
            format!("{e} -- GRUB's scripts belong to the administrator: run this as root")
        }
        _ => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it -- that is the diagnosis. The defensive lints exist to keep panics
    // out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicU32, Ordering};

    // -- a scratch system -------------------------------------------------

    /// A directory standing in for a system's root, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "slateos_grubcmd_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            // A leftover from a killed run is stale scratch, not data.
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// A root with GRUB's menu at `cfg` and its scripts directory.
        fn with_grub(cfg: &str) -> Self {
            let scratch = Self::new();
            scratch.write(cfg, "set timeout=5\nset default=\"0\"\n");
            fs::create_dir_all(scratch.0.join("etc/grub.d")).unwrap();
            scratch
        }

        /// The same, on a running system started through UEFI.
        fn efi() -> Self {
            let scratch = Self::with_grub("boot/grub/grub.cfg");
            fs::create_dir_all(scratch.0.join("sys/firmware/efi")).unwrap();
            scratch
        }

        fn write(&self, rel: &str, contents: impl AsRef<[u8]>) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn read(&self, rel: &str) -> Option<String> {
            fs::read_to_string(self.0.join(rel)).ok()
        }

        fn script(&self) -> Option<String> {
            self.read("etc/grub.d/40_slateos")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Scratch space under the temp directory: a failure leaves a
            // directory behind, which harms nothing.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// A command for `root`, parsed from the flag's options.
    fn command(verb: Verb, root: &Scratch, rest: &[&str]) -> Command {
        let mut args = os(rest);
        args.push("--root".into());
        args.push(root.0.clone().into_os_string());
        parse(verb, &args).unwrap()
    }

    /// Run with a stand-in host: `live` as given, `update-grub` as its tool,
    /// and a rebuild that does what `rebuild` says, counted in `calls`.
    fn run_with(
        cmd: &Command,
        live: bool,
        calls: &Cell<u32>,
        rebuild: &dyn Fn(&GrubInstall) -> Result<&'static str, GrubError>,
    ) -> Result<String, String> {
        let counted = |install: &GrubInstall| {
            calls.set(calls.get() + 1);
            rebuild(install)
        };
        let tool = || Some("update-grub");
        run_on(
            cmd,
            &Host {
                live,
                tool: &tool,
                rebuild: &counted,
            },
        )
    }

    /// What `grub-mkconfig` does with the scripts: the menu fences each
    /// script it ran, and it runs ours if it is there.
    fn mkconfig(install: &GrubInstall) -> Result<&'static str, GrubError> {
        let root = install.config_path.ancestors().nth(3).unwrap();
        let mut cfg = String::from("set timeout=5\n");
        if root.join("etc/grub.d/40_slateos").exists() {
            cfg.push_str("### BEGIN /etc/grub.d/40_slateos ###\nmenuentry \"Slate OS\" {}\n");
            cfg.push_str("### END /etc/grub.d/40_slateos ###\n");
        }
        fs::write(&install.config_path, cfg)?;
        Ok("update-grub")
    }

    fn never(_: &GrubInstall) -> Result<&'static str, GrubError> {
        panic!("the menu was rebuilt when it should not have been")
    }

    const UUID: &str = "1A2B-3C4D";

    // -- reading the options ----------------------------------------------

    #[test]
    fn the_entry_chainloads_limine_from_its_own_directory_by_default() {
        let cmd = parse(Verb::Add, &os(&["--uuid", UUID])).unwrap();
        assert_eq!(cmd.root, PathBuf::from("/"));
        assert!(cmd.rebuild);
        let Action::Add(entry) = cmd.action else {
            panic!("--grub-add built {:?}", cmd.action)
        };
        assert_eq!(entry, grub_entry(GRUB_TITLE, UUID));
        assert_eq!(entry.kernel_path, LIMINE_EFI_PATH);
        assert_eq!(entry.entry_type, grub::GrubEntryType::Chainload);
    }

    #[test]
    fn every_command_keeps_what_it_was_told() {
        let cmd = parse(
            Verb::Update,
            &os(&[
                "--title",
                "Slate",
                "--path",
                "/EFI/BOOT/BOOTX64.EFI",
                "--uuid",
                UUID,
                "--root",
                "/mnt/linux",
                "--no-update",
            ]),
        )
        .unwrap();
        assert_eq!(cmd.root, PathBuf::from("/mnt/linux"));
        assert!(!cmd.rebuild);
        let Action::Update(entry) = cmd.action else {
            panic!("--grub-update built {:?}", cmd.action)
        };
        assert_eq!(
            (
                entry.title.as_str(),
                entry.kernel_path.as_str(),
                entry.uuid.as_str()
            ),
            ("Slate", "/EFI/BOOT/BOOTX64.EFI", UUID)
        );

        let cmd = parse(Verb::Remove, &os(&["--no-update", "--root", "/mnt"])).unwrap();
        assert_eq!(
            (cmd.action, cmd.root, cmd.rebuild),
            (Action::Remove, PathBuf::from("/mnt"), false)
        );
        let cmd = parse(Verb::Detect, &os(&[])).unwrap();
        assert_eq!((cmd.action, cmd.root), (Action::Detect, PathBuf::from("/")));
    }

    #[test]
    fn adding_needs_the_partitions_uuid_and_a_real_one() {
        for verb in [Verb::Add, Verb::Update] {
            let err = parse(verb, &os(&[])).unwrap_err();
            assert!(err.contains("--uuid") && err.contains(verb.flag()), "{err}");
        }
        let err = parse(Verb::Add, &os(&["--uuid", "not-a-uuid"])).unwrap_err();
        assert!(
            err.contains("'not-a-uuid' is not a filesystem UUID"),
            "{err}"
        );
    }

    #[test]
    fn an_option_a_command_does_not_use_is_refused() {
        for (verb, option) in [
            (Verb::Remove, "--uuid"),
            (Verb::Remove, "--path"),
            (Verb::Detect, "--title"),
        ] {
            let err = parse(verb, &os(&[option, "x"])).unwrap_err();
            assert_eq!(err, format!("{option} means nothing to {}", verb.flag()));
        }
        let err = parse(Verb::Detect, &os(&["--no-update"])).unwrap_err();
        assert!(err.contains("--no-update means nothing"), "{err}");
        let err = parse(Verb::Remove, &os(&["--force"])).unwrap_err();
        assert_eq!(err, "unknown option '--force' for --grub-remove");
    }

    #[test]
    fn an_option_given_twice_or_without_its_value_is_refused() {
        let err = parse(Verb::Add, &os(&["--uuid", UUID, "--uuid", UUID])).unwrap_err();
        assert_eq!(err, "--uuid given twice");
        let err = parse(Verb::Remove, &os(&["--no-update", "--no-update"])).unwrap_err();
        assert_eq!(err, "--no-update given twice");
        for option in ["--root", "--uuid", "--path", "--title"] {
            let err = parse(Verb::Add, &os(&[option])).unwrap_err();
            assert_eq!(err, format!("{option} needs a value"));
        }
    }

    #[test]
    fn the_direct_strategy_is_refused_with_the_reason() {
        let err = parse(Verb::Add, &os(&["--uuid", UUID, "--direct"])).unwrap_err();
        assert!(
            err.contains("multiboot2") && err.contains("chainloads"),
            "{err}"
        );
    }

    #[test]
    fn an_entry_grub_could_not_show_is_refused_before_anything_is_found() {
        let err = parse(
            Verb::Add,
            &os(&["--uuid", UUID, "--path", "EFI/limine.efi"]),
        )
        .unwrap_err();
        assert!(err.contains("must start at the partition's root"), "{err}");
        let err = parse(Verb::Add, &os(&["--uuid", UUID, "--title", "  "])).unwrap_err();
        assert!(err.contains("--title is empty"), "{err}");
        let err = parse(
            Verb::Add,
            &os(&["--uuid", UUID, "--title", "Slate\nmenuentry"]),
        )
        .unwrap_err();
        assert!(err.contains("control character"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_root_that_is_not_text_is_kept_as_it_is() {
        use std::os::unix::ffi::OsStrExt;
        let root = OsStr::from_bytes(b"/mnt/caf\xe9").to_os_string();
        let cmd = parse(Verb::Detect, &[OsString::from("--root"), root.clone()]).unwrap();
        assert_eq!(cmd.root.as_os_str(), root);
        let err = parse(Verb::Add, &[OsString::from("--uuid"), root]).unwrap_err();
        assert!(
            err.contains("caf\\351") && err.contains("is not text"),
            "{err}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_root_that_is_not_text_is_kept_as_it_is() {
        use std::os::windows::ffi::OsStringExt;
        let mut wide: Vec<u16> = "C:\\caf".encode_utf16().collect();
        wide.push(0xD800);
        let root = OsString::from_wide(&wide);
        let cmd = parse(Verb::Detect, &[OsString::from("--root"), root.clone()]).unwrap();
        assert_eq!(cmd.root.as_os_str(), root);
        let err = parse(Verb::Add, &[OsString::from("--uuid"), root]).unwrap_err();
        assert!(err.contains("is not text"), "{err}");
    }

    // -- --grub-detect ----------------------------------------------------

    #[test]
    fn detect_reports_what_it_finds() {
        let root = Scratch::efi();
        root.write("boot/grub/grub.cfg", "set timeout=7\nset default=\"2\"\n");
        root.write("etc/default/grub", "GRUB_DISABLE_OS_PROBER=true\n");
        root.write(SECURE_BOOT_VAR, [6, 0, 0, 0, 1]);
        let calls = Cell::new(0);
        let said = run_with(&command(Verb::Detect, &root, &[]), true, &calls, &never).unwrap();
        for line in [
            "GRUB 2: ",
            "  Firmware:       UEFI",
            "  Secure Boot:    on -- GRUB will not start Limine until it is enrolled",
            "  Slate OS entry: none",
            "  Timeout:        7 s",
            "  Default entry:  2",
            "  os-prober:      off",
            "  Rebuilt by:     update-grub",
        ] {
            assert!(said.contains(line), "no {line:?} in:\n{said}");
        }
        assert!(
            said.contains("grub.cfg") && said.contains("grub.d"),
            "{said}"
        );
        assert_eq!(calls.get(), 0, "detecting rebuilt the menu");
    }

    #[test]
    fn detect_says_what_it_can_tell_about_the_firmware_and_no_more() {
        let root = Scratch::with_grub("boot/grub/grub.cfg");
        let calls = Cell::new(0);
        let cmd = command(Verb::Detect, &root, &[]);
        // Another system's root, mounted without its /sys: nothing to tell,
        // and no tool of this machine's to name.
        let said = run_with(&cmd, false, &calls, &never).unwrap();
        assert!(said.contains("Firmware:       not known"), "{said}");
        assert!(
            !said.contains("Secure Boot") && !said.contains("Rebuilt by"),
            "{said}"
        );
        // The running system without /sys/firmware/efi was started through
        // the BIOS.
        let said = run_with(&cmd, true, &calls, &never).unwrap();
        assert!(said.contains("Firmware:       BIOS"), "{said}");
        root.write(SECURE_BOOT_VAR, [6, 0, 0, 0, 0]);
        let said = run_with(&cmd, false, &calls, &never).unwrap();
        assert!(said.contains("Secure Boot:    off"), "{said}");
    }

    #[test]
    fn detect_tells_an_entry_in_the_menu_from_one_waiting_for_a_rebuild() {
        let root = Scratch::efi();
        let calls = Cell::new(0);
        let detect = command(Verb::Detect, &root, &[]);
        let entry = || {
            let said = run_with(&detect, false, &calls, &never).unwrap();
            let line = said
                .lines()
                .find(|l| l.contains("Slate OS entry:"))
                .unwrap();
            line.trim_start_matches("  Slate OS entry: ").to_string()
        };
        assert_eq!(entry(), "none");
        run_with(
            &command(Verb::Add, &root, &["--uuid", UUID, "--no-update"]),
            true,
            &calls,
            &never,
        )
        .unwrap();
        assert_eq!(entry(), "written; in the menu once it is rebuilt");
        let install = GrubDetector::with_root(&root.0).detect().unwrap();
        mkconfig(&install).unwrap();
        assert_eq!(entry(), "in the menu");
        fs::remove_file(root.0.join("etc/grub.d/40_slateos")).unwrap();
        assert_eq!(entry(), "removed; in the menu until it is rebuilt");
        root.write("etc/grub.d/40_slateos", "#!/bin/sh\n");
        assert!(entry().ends_with("is someone else's"), "{}", entry());
    }

    #[test]
    fn without_grub_every_command_says_so_and_names_the_root() {
        let root = Scratch::new();
        let calls = Cell::new(0);
        for verb in [Verb::Detect, Verb::Remove] {
            let err = run_with(&command(verb, &root, &[]), false, &calls, &never).unwrap_err();
            assert!(err.starts_with("no GRUB found under "), "{err}");
            assert!(err.contains(&root.0.shown().to_string()), "{err}");
        }
    }

    // -- --grub-add, --grub-update, --grub-remove -------------------------

    #[test]
    fn adding_to_another_systems_root_leaves_the_rebuild_to_that_system() {
        let root = Scratch::with_grub("boot/grub2/grub.cfg");
        let calls = Cell::new(0);
        let cmd = command(Verb::Add, &root, &["--uuid", UUID, "--title", "Slate"]);
        let said = run_with(&cmd, false, &calls, &never).unwrap();
        assert_eq!(calls.get(), 0);
        let script = root.script().unwrap();
        assert!(script.contains("menuentry \"Slate\""), "{script}");
        assert!(
            script.contains(&format!("--set=root \"{UUID}\"")),
            "{script}"
        );
        assert!(
            script.contains(&format!("chainloader \"{LIMINE_EFI_PATH}\"")),
            "{script}"
        );
        assert!(
            said.starts_with("Wrote the Slate OS entry, 'Slate', chainloading"),
            "{said}"
        );
        // The menu's path as that system sees it, and the tool its path says
        // it has.
        assert!(
            said.contains(
                "start it and run update-grub (or grub2-mkconfig -o /boot/grub2/grub.cfg) there"
            ),
            "{said}"
        );
    }

    #[test]
    fn adding_to_the_running_system_rebuilds_its_menu_and_reads_it_back() {
        let root = Scratch::efi();
        let calls = Cell::new(0);
        let seen = RefCell::new(PathBuf::new());
        let rebuild = |install: &GrubInstall| {
            *seen.borrow_mut() = install.config_path.clone();
            mkconfig(install)
        };
        let said = run_with(
            &command(Verb::Add, &root, &["--uuid", UUID]),
            true,
            &calls,
            &rebuild,
        )
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(*seen.borrow(), root.0.join("boot/grub/grub.cfg"));
        assert!(
            said.ends_with("update-grub rebuilt GRUB's menu; the entry is in it.\n"),
            "{said}"
        );

        let said = run_with(&command(Verb::Remove, &root, &[]), true, &calls, &rebuild).unwrap();
        assert_eq!(calls.get(), 2);
        assert!(root.script().is_none());
        assert!(said.ends_with("the entry is gone from it.\n"), "{said}");
    }

    #[test]
    fn a_rebuild_that_leaves_the_entry_out_is_an_error() {
        // grub-mkconfig skips a script that is not executable and still
        // succeeds; the menu read back is what shows it.
        let root = Scratch::efi();
        let calls = Cell::new(0);
        let skips = |install: &GrubInstall| -> Result<&'static str, GrubError> {
            fs::write(&install.config_path, "set timeout=5\n")?;
            Ok("grub2-mkconfig")
        };
        let err = run_with(
            &command(Verb::Add, &root, &["--uuid", UUID]),
            true,
            &calls,
            &skips,
        )
        .unwrap_err();
        assert!(err.starts_with("Wrote the Slate OS entry"), "{err}");
        assert!(err.ends_with("grub2-mkconfig rebuilt GRUB's menu, but the entry is not in it: is the script executable?"), "{err}");

        let keeps = |install: &GrubInstall| -> Result<&'static str, GrubError> {
            fs::write(
                &install.config_path,
                "### BEGIN /etc/grub.d/40_slateos ###\n",
            )?;
            Ok("update-grub")
        };
        let err = run_with(&command(Verb::Remove, &root, &[]), true, &calls, &keeps).unwrap_err();
        assert!(err.ends_with("but the entry is still in it"), "{err}");
    }

    #[test]
    fn no_update_leaves_the_running_systems_menu_alone() {
        let root = Scratch::efi();
        let calls = Cell::new(0);
        let cmd = command(Verb::Add, &root, &["--uuid", UUID, "--no-update"]);
        let said = run_with(&cmd, true, &calls, &never).unwrap();
        assert!(root.script().is_some());
        assert!(
            said.ends_with("GRUB's menu has the change once it is rebuilt: run update-grub (or grub-mkconfig -o /boot/grub/grub.cfg).\n"),
            "{said}"
        );
    }

    #[test]
    fn a_rebuild_that_cannot_happen_says_the_entry_is_written_but_not_in_the_menu() {
        let root = Scratch::efi();
        let calls = Cell::new(0);
        let cmd = command(Verb::Add, &root, &["--uuid", UUID]);
        let missing =
            |_: &GrubInstall| -> Result<&'static str, GrubError> { Err(GrubError::GrubNotFound) };
        let err = run_with(&cmd, true, &calls, &missing).unwrap_err();
        assert!(err.starts_with("Wrote the Slate OS entry"), "{err}");
        assert!(
            err.contains("No update-grub or grub-mkconfig was found"),
            "{err}"
        );

        let cmd = command(Verb::Update, &root, &["--uuid", UUID]);
        let fails = |_: &GrubInstall| -> Result<&'static str, GrubError> {
            Err(GrubError::UpdateFailed("exit 1".to_string()))
        };
        let err = run_with(&cmd, true, &calls, &fails).unwrap_err();
        assert!(err.starts_with("Rewrote the Slate OS entry"), "{err}");
        assert!(
            err.ends_with("Rebuilding GRUB's menu failed: GRUB update failed: exit 1"),
            "{err}"
        );
    }

    #[test]
    fn a_machine_started_through_the_bios_cannot_chainload_limine() {
        let root = Scratch::with_grub("boot/grub/grub.cfg");
        let calls = Cell::new(0);
        let err = run_with(
            &command(Verb::Add, &root, &["--uuid", UUID]),
            true,
            &calls,
            &never,
        )
        .unwrap_err();
        assert!(err.contains("started through the BIOS"), "{err}");
        assert!(root.script().is_none(), "the refused entry was written");
        // Removing an entry needs no chainloading, and is not refused.
        root.write(
            "etc/grub.d/40_slateos",
            grub::generate_custom_script(&grub_entry("S", UUID)).unwrap(),
        );
        run_with(
            &command(Verb::Remove, &root, &["--no-update"]),
            true,
            &calls,
            &never,
        )
        .unwrap();
        assert!(root.script().is_none());
    }

    #[test]
    fn each_command_points_at_the_one_that_fits() {
        let root = Scratch::efi();
        let calls = Cell::new(0);
        let add = command(Verb::Add, &root, &["--uuid", UUID]);
        let err = run_with(
            &command(Verb::Update, &root, &["--uuid", UUID]),
            false,
            &calls,
            &never,
        )
        .unwrap_err();
        assert!(err.ends_with("to rewrite; --grub-add adds one"), "{err}");
        let err = run_with(&command(Verb::Remove, &root, &[]), false, &calls, &never).unwrap_err();
        assert!(err.ends_with("to remove; --grub-add adds one"), "{err}");
        run_with(&add, false, &calls, &never).unwrap();
        let err = run_with(&add, false, &calls, &never).unwrap_err();
        assert!(err.ends_with("; --grub-update rewrites it"), "{err}");
    }

    #[test]
    fn update_rewrites_the_entry_and_remove_removes_it() {
        let root = Scratch::efi();
        let calls = Cell::new(0);
        run_with(
            &command(Verb::Add, &root, &["--uuid", UUID]),
            false,
            &calls,
            &never,
        )
        .unwrap();
        let cmd = command(
            Verb::Update,
            &root,
            &["--uuid", "ABCD-1234", "--title", "Slate 2"],
        );
        let said = run_with(&cmd, false, &calls, &never).unwrap();
        assert!(
            said.starts_with("Rewrote the Slate OS entry, 'Slate 2'"),
            "{said}"
        );
        let script = root.script().unwrap();
        assert!(
            script.contains("Slate 2") && script.contains("ABCD-1234"),
            "{script}"
        );
        assert!(!script.contains(UUID), "{script}");
        let said = run_with(&command(Verb::Remove, &root, &[]), false, &calls, &never).unwrap();
        assert!(said.starts_with("Removed the Slate OS entry"), "{said}");
        assert!(root.script().is_none());
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn a_script_of_our_name_that_is_someone_elses_is_left_alone() {
        let root = Scratch::efi();
        let calls = Cell::new(0);
        root.write("etc/grub.d/40_slateos", "#!/bin/sh\necho mine\n");
        for cmd in [
            command(Verb::Add, &root, &["--uuid", UUID]),
            command(Verb::Update, &root, &["--uuid", UUID]),
            command(Verb::Remove, &root, &[]),
        ] {
            let err = run_with(&cmd, false, &calls, &never).unwrap_err();
            assert!(
                err.ends_with("move it aside for the installer to write its own"),
                "{err}"
            );
        }
        assert_eq!(root.script().unwrap(), "#!/bin/sh\necho mine\n");
    }

    #[test]
    fn without_scripts_the_entry_is_given_to_be_added_by_hand() {
        let root = Scratch::new();
        root.write("boot/grub/grub.cfg", "set timeout=5\n");
        let calls = Cell::new(0);
        let err = run_with(
            &command(Verb::Add, &root, &["--uuid", UUID]),
            false,
            &calls,
            &never,
        )
        .unwrap_err();
        assert!(err.contains("has no etc/grub.d"), "{err}");
        assert!(err.contains("menuentry \"Slate OS\""), "{err}");
        assert!(
            err.contains(&format!("chainloader \"{LIMINE_EFI_PATH}\"")),
            "{err}"
        );
        let err = run_with(&command(Verb::Remove, &root, &[]), false, &calls, &never).unwrap_err();
        assert!(
            err.ends_with("so it has no Slate OS script to remove"),
            "{err}"
        );
    }

    #[test]
    fn secure_boot_on_is_said_when_an_entry_is_written() {
        let root = Scratch::efi();
        root.write(SECURE_BOOT_VAR, [6, 0, 0, 0, 1]);
        let calls = Cell::new(0);
        let said = run_with(
            &command(Verb::Add, &root, &["--uuid", UUID]),
            false,
            &calls,
            &never,
        )
        .unwrap();
        assert!(
            said.contains("Secure Boot is on: GRUB will not start Limine"),
            "{said}"
        );
        root.write(SECURE_BOOT_VAR, [6, 0, 0, 0, 0]);
        let said = run_with(
            &command(Verb::Update, &root, &["--uuid", UUID]),
            false,
            &calls,
            &never,
        )
        .unwrap();
        assert!(!said.contains("Secure Boot"), "{said}");
    }

    #[test]
    fn run_takes_a_root_other_than_slash_for_another_systems() {
        // `run` is what the command line calls, with this machine's GRUB
        // tools. Under a root other than `/` nothing is rebuilt. The root was
        // started through the BIOS, so were `run` to take it for the running
        // system it would refuse the entry before writing or running
        // anything -- a mistake shows here without touching this machine.
        let root = Scratch::with_grub("boot/grub/grub.cfg");
        let said = run(&command(Verb::Add, &root, &["--uuid", UUID])).unwrap();
        assert!(
            said.contains("That system's menu has the change once it rebuilds it"),
            "{said}"
        );
        assert!(root.script().is_some());
    }

    #[test]
    fn a_script_the_user_may_not_write_asks_for_root() {
        let e = GrubError::Io(io::Error::from(io::ErrorKind::PermissionDenied));
        let said = explain(&e, &Action::Remove, Path::new("/etc/grub.d"));
        assert!(said.ends_with("run this as root"), "{said}");
    }
}
