"""Mutation test for the installer's bootloader choice and its GRUB commands.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-28, when the GRUB module was wired
up (C-Q17, design-decisions §1423).  `grub.rs` had been complete and reached
from nothing: the installer could neither be told to use GRUB nor asked to
add Slate OS to one.  Now a configuration's `bootloader:` section chooses
Limine or a GRUB entry, which the plan adds after Limine; and `--grub-detect`,
`--grub-add`, `--grub-update` and `--grub-remove` find a GRUB and change the
entry, which chainloads Limine -- the kernel has no multiboot2 header for GRUB
to load it by itself.  Along the way `grub.rs` stopped writing over, or
deleting, a script of its name that it did not write, and stopped asking an
external `which` for GRUB's tools.

Not swept: running a GRUB tool.  `GrubUpdateRunner::update_grub` and
`detect_command` search the real `PATH` and run what they find; a test that
did either would be rebuilding this machine's boot menu.  The search itself is
`find_in`, which is swept, and the commands' rebuild is the `Host` the tests
stand in for.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# ── lib.rs: the configuration and the plan ─────────────────────────────────
LIB = [
    (
        "a grub section is read as Limine",
        "                Ok(BootloaderConfig::Grub { title })",
        "                Ok(BootloaderConfig::Limine)",
        [
            "a_grub_section_is_read_with_its_title",
            "the_plan_adds_the_grub_entry_right_after_limine",
            "the_sample_config_shows_how_to_choose_grub",
        ],
    ),
    (
        "a grub section without a title is titled nothing",
        "                    .unwrap_or(GRUB_TITLE)",
        "                    .unwrap_or(\"\")",
        ["a_grub_section_is_read_with_its_title"],
    ),
    (
        "GRUB is let load the kernel itself",
        "                if !strategy.eq_ignore_ascii_case(\"chainload\") {",
        "                if false {",
        ["grub_loading_the_kernel_itself_is_refused_with_the_reason"],
    ),
    (
        "a strategy's case matters",
        "                if !strategy.eq_ignore_ascii_case(\"chainload\") {",
        "                if strategy != \"chainload\" {",
        ["a_grub_section_is_read_with_its_title"],
    ),
    (
        "a bootloader the installer does not know is taken for Limine",
        "            other => Err(ConfigError::InvalidValue {\n"
        "                field: \"bootloader.type\".to_string(),\n"
        "                message: format!(\"unsupported bootloader '{other}' (limine or grub)\"),\n"
        "            }),",
        "            _ => Ok(BootloaderConfig::Limine),",
        ["a_bootloader_the_installer_does_not_know_is_refused"],
    ),
    (
        "a bootloader's type is read as written",
        "        match kind.to_ascii_lowercase().as_str() {",
        "        match kind {",
        ["a_grub_section_is_read_with_its_title"],
    ),
    (
        "no bootloader section is refused",
        "        let Some(section) = root.get(\"bootloader\") else {\n"
        "            return Ok(BootloaderConfig::Limine);",
        "        let Some(section) = root.get(\"bootloader\") else {\n"
        "            return Err(ConfigError::MissingField(\"bootloader\".to_string()));",
        ["without_a_bootloader_section_limine_starts_the_system"],
    ),
    (
        "a section without a type is refused",
        "            .unwrap_or(\"limine\");",
        "            .unwrap_or(\"none\");",
        ["without_a_bootloader_section_limine_starts_the_system"],
    ),
    (
        "a title GRUB could not show is taken",
        "grub_entry(title, \"0000-0000\").validate()",
        "grub_entry(\"\", \"0000-0000\").validate()",
        ["a_grub_title_grub_could_not_show_is_refused"],
    ),
    (
        "the plan leaves the GRUB entry out",
        "            steps.push(InstallStep::AddGrubEntry {\n"
        "                title: title.clone(),\n"
        "            });",
        "            let _ = title;",
        ["the_plan_adds_the_grub_entry_right_after_limine"],
    ),
    (
        "the GRUB entry is planned before Limine is installed",
        "        steps.push(InstallStep::InstallBootloader {\n"
        "            target: config.disk.target.clone(),\n"
        "        });\n"
        "        if let BootloaderConfig::Grub { title } = &config.bootloader {\n"
        "            steps.push(InstallStep::AddGrubEntry {\n"
        "                title: title.clone(),\n"
        "            });\n"
        "        }",
        "        if let BootloaderConfig::Grub { title } = &config.bootloader {\n"
        "            steps.push(InstallStep::AddGrubEntry {\n"
        "                title: title.clone(),\n"
        "            });\n"
        "        }\n"
        "        steps.push(InstallStep::InstallBootloader {\n"
        "            target: config.disk.target.clone(),\n"
        "        });",
        ["the_plan_adds_the_grub_entry_right_after_limine"],
    ),
    (
        "the plan does not say what GRUB chainloads",
        "format!(\"Add '{title}' to GRUB's menu, chainloading {LIMINE_EFI_PATH}\")",
        "format!(\"Add '{title}' to GRUB's menu\")",
        ["the_plan_adds_the_grub_entry_right_after_limine"],
    ),
    (
        "GRUB is to load the kernel itself",
        "        entry_type: grub::GrubEntryType::Chainload,",
        "        entry_type: grub::GrubEntryType::Direct,",
        ["the_grub_entry_chainloads_limine_by_the_partitions_uuid"],
    ),
    (
        "GRUB chainloads the firmware's fallback path",
        "        kernel_path: LIMINE_EFI_PATH.to_string(),",
        "        kernel_path: \"/EFI/BOOT/BOOTX64.EFI\".to_string(),",
        ["the_grub_entry_chainloads_limine_by_the_partitions_uuid"],
    ),
    (
        "the partition is not found by its UUID",
        "        uuid: esp_uuid.to_string(),",
        "        uuid: String::new(),",
        ["the_grub_entry_chainloads_limine_by_the_partitions_uuid"],
    ),
    (
        "the sample config does not show the choice",
        "bootloader:\n  type: limine\n#  type: grub\n#  title: Slate OS\n",
        "",
        ["the_sample_config_shows_how_to_choose_grub"],
    ),
]

# ── grub.rs: whose script it is, and finding GRUB's tools ─────────────────
GRUB = [
    (
        "install writes over a script that is someone else's",
        "            EntryState::Foreign => return Err(GrubError::NotOurs(path.shown().to_string())),",
        "            EntryState::Foreign => {}",
        ["a_script_of_our_name_that_is_not_ours_is_left_alone"],
    ),
    (
        "update and uninstall act on a script that is someone else's",
        "            EntryState::Foreign => Err(GrubError::NotOurs(path.shown().to_string())),",
        "            EntryState::Foreign => Ok(()),",
        ["a_script_of_our_name_that_is_not_ours_is_left_alone"],
    ),
    (
        "the marker is not looked for",
        "        Ok(if bytes.windows(marker.len()).any(|w| w == marker) {",
        "        Ok(if !bytes.is_empty() {",
        [
            "a_script_of_our_name_that_is_not_ours_is_left_alone",
            "a_script_that_is_not_text_is_someone_elses_rather_than_an_error",
        ],
    ),
    (
        "a script that is not text is an error",
        "        let bytes = match fs::read(self.script_path()) {",
        "        let bytes = match fs::read_to_string(self.script_path()).map(String::into_bytes) {",
        ["a_script_that_is_not_text_is_someone_elses_rather_than_an_error"],
    ),
    (
        "no script at all is an error",
        "            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(EntryState::Absent),",
        "            Err(e) if false && e.kind() == io::ErrorKind::NotFound => return Ok(EntryState::Absent),",
        ["the_entrys_state_follows_its_lifecycle", "test_installer_uninstall_not_found"],
    ),
    (
        "a relative directory on PATH is searchable",
        "fn searchable(dir: &Path) -> bool {\n    dir.is_absolute()\n}",
        "fn searchable(dir: &Path) -> bool {\n    let _ = dir;\n    true\n}",
        [
            "only_an_absolute_directory_on_path_is_searched",
            "a_directory_on_path_that_is_not_absolute_is_passed_over",
        ],
    ),
    (
        # The relative entry finds the crate's own Cargo.toml only where any
        # file can be run -- the Windows host the sweep runs on.
        "PATH is searched without asking which directories may be",
        "        .filter(|dir| searchable(dir))",
        "        .filter(|_| true)",
        ["a_directory_on_path_that_is_not_absolute_is_passed_over"],
    ),
    (
        "a directory of the program's name is taken for it",
        "        .find(|candidate| is_executable(candidate))",
        "        .find(|candidate| candidate.exists())",
        ["a_directory_of_the_programs_name_is_not_the_program"],
    ),
    (
        "the last directory on PATH wins",
        "        .find(|candidate| is_executable(candidate))",
        "        .filter(|candidate| is_executable(candidate))\n        .last()",
        ["a_program_is_found_in_the_first_directory_that_has_it"],
    ),
]

# ── grubcmd.rs: the GRUB commands ──────────────────────────────────────────
CMD = [
    (
        "--root is not the command's root",
        "        root: root.unwrap_or_else(|| PathBuf::from(\"/\")),",
        "        root: PathBuf::from(\"/\"),",
        ["every_command_keeps_what_it_was_told"],
    ),
    (
        "--no-update is ignored",
        "        rebuild: !no_update,",
        "        rebuild: true,",
        [
            "every_command_keeps_what_it_was_told",
            "no_update_leaves_the_running_systems_menu_alone",
        ],
    ),
    (
        "an option a command does not use is ignored",
        "    if !verb.writes_entry() {",
        "    if false {",
        ["an_option_a_command_does_not_use_is_refused"],
    ),
    (
        "--direct is taken",
        "            Some(\"--direct\") => return Err(DIRECT_REFUSED.to_string()),",
        "            Some(\"--direct\") => {}",
        ["the_direct_strategy_is_refused_with_the_reason"],
    ),
    (
        "an option given twice keeps the last",
        "    if slot.is_some() {\n        return Err(format!(\"{option} given twice\"));\n    }\n",
        "",
        ["an_option_given_twice_or_without_its_value_is_refused"],
    ),
    (
        "any UUID is taken",
        "    if !grub::is_valid_uuid(&uuid) {",
        "    if false {",
        ["adding_needs_the_partitions_uuid_and_a_real_one"],
    ),
    (
        "an empty title is taken",
        "    if title.trim().is_empty() {",
        "    if false {",
        ["an_entry_grub_could_not_show_is_refused_before_anything_is_found"],
    ),
    (
        "a path from nowhere in particular is taken",
        "    if !path.starts_with('/') {",
        "    if false {",
        ["an_entry_grub_could_not_show_is_refused_before_anything_is_found"],
    ),
    (
        "--path is ignored",
        "    entry.kernel_path = path;",
        "    let _ = path;",
        ["every_command_keeps_what_it_was_told"],
    ),
    (
        "an entry GRUB could not show is found out only when written",
        "    entry.validate().map_err(|e| e.to_string())?;\n",
        "",
        ["an_entry_grub_could_not_show_is_refused_before_anything_is_found"],
    ),
    (
        "--grub-add and --grub-update are the other way round",
        "            if verb == Verb::Add {",
        "            if verb == Verb::Update {",
        [
            "every_command_keeps_what_it_was_told",
            "the_entry_chainloads_limine_from_its_own_directory_by_default",
        ],
    ),
    (
        "a machine started through the BIOS is let add the entry",
        "    if entry.is_some() && host.live && !install.is_efi() {",
        "    if false {",
        ["a_machine_started_through_the_bios_cannot_chainload_limine"],
    ),
    (
        "removing the entry is refused on a machine started through the BIOS",
        "    if entry.is_some() && host.live && !install.is_efi() {",
        "    if host.live && !install.is_efi() {",
        ["a_machine_started_through_the_bios_cannot_chainload_limine"],
    ),
    (
        "another system's root is rebuilt with this machine's tools",
        "    if !(host.live && cmd.rebuild) {",
        "    if !cmd.rebuild {",
        ["adding_to_another_systems_root_leaves_the_rebuild_to_that_system"],
    ),
    (
        "the rebuilt menu is not read back",
        "    if in_menu(install) != wanted {",
        "    if false {",
        ["a_rebuild_that_leaves_the_entry_out_is_an_error"],
    ),
    (
        "no tool to rebuild with reads as a tool that failed",
        "        Err(GrubError::GrubNotFound) => {",
        "        Err(GrubError::GrubNotFound) if false => {",
        ["a_rebuild_that_cannot_happen_says_the_entry_is_written_but_not_in_the_menu"],
    ),
    (
        "Secure Boot is not mentioned when an entry is written",
        "    if entry.is_some() && secure_boot(&cmd.root) == Some(true) {",
        "    if false {",
        ["secure_boot_on_is_said_when_an_entry_is_written"],
    ),
    (
        "Secure Boot off is read as on",
        "    bytes.get(4).map(|&on| on == 1)",
        "    bytes.get(4).map(|_| true)",
        [
            "detect_says_what_it_can_tell_about_the_firmware_and_no_more",
            "secure_boot_on_is_said_when_an_entry_is_written",
        ],
    ),
    (
        "another system's firmware is guessed at",
        "    } else if live {\n        \"BIOS -- GRUB started this way cannot chainload Limine\"",
        "    } else if true {\n        \"BIOS -- GRUB started this way cannot chainload Limine\"",
        ["detect_says_what_it_can_tell_about_the_firmware_and_no_more"],
    ),
    (
        "the menu is searched for another script's fence",
        "format!(\"### BEGIN /etc/grub.d/{} ###\", grub::CUSTOM_SCRIPT_NAME)",
        "format!(\"### BEGIN /etc/grub.d/{} ###\", \"41_custom\")",
        [
            "detect_tells_an_entry_in_the_menu_from_one_waiting_for_a_rebuild",
            "adding_to_the_running_system_rebuilds_its_menu_and_reads_it_back",
        ],
    ),
    (
        "an entry waiting for a rebuild is said to be in the menu",
        "                Ok(EntryState::Ours) if in_menu => \"in the menu\".to_string(),",
        "                Ok(EntryState::Ours) if true => \"in the menu\".to_string(),",
        ["detect_tells_an_entry_in_the_menu_from_one_waiting_for_a_rebuild"],
    ),
    (
        "another system is told this machine's path to its menu",
        "        |rel| Path::new(\"/\").join(rel),",
        "        |_| install.config_path.clone(),",
        ["adding_to_another_systems_root_leaves_the_rebuild_to_that_system"],
    ),
    (
        "a grub2 system is told grub-mkconfig",
        "    let mkconfig = if config.contains(\"grub2\") {",
        "    let mkconfig = if false {",
        ["adding_to_another_systems_root_leaves_the_rebuild_to_that_system"],
    ),
    (
        "an entry that is there is not pointed at --grub-update",
        "            \"Slate OS already has an entry in {}; --grub-update rewrites it\",",
        "            \"Slate OS already has an entry in {}\",",
        ["each_command_points_at_the_one_that_fits"],
    ),
    (
        "a rewrite and a removal are told the same",
        "            if matches!(action, Action::Remove) {",
        "            if false {",
        ["each_command_points_at_the_one_that_fits"],
    ),
    (
        "a refusal to write is not put down to permissions",
        "        GrubError::Io(io) if io.kind() == io::ErrorKind::PermissionDenied => {",
        "        GrubError::Io(io) if false && io.kind() == io::ErrorKind::PermissionDenied => {",
        ["a_script_the_user_may_not_write_asks_for_root"],
    ),
    (
        "without scripts the entry is not given to be added by hand",
        "                let block = grub::generate_entry(entry).map_err(|e| e.to_string())?;",
        "                let block = String::new();",
        ["without_scripts_the_entry_is_given_to_be_added_by_hand"],
    ),
    (
        "an update is written as a fresh entry",
        "        Action::Update(entry) => installer.update(entry),",
        "        Action::Update(entry) => installer.install(entry),",
        ["update_rewrites_the_entry_and_remove_removes_it"],
    ),
    (
        "every root is taken for the running system's",
        "        live: cmd.root == Path::new(\"/\"),",
        "        live: true,",
        ["run_takes_a_root_other_than_slash_for_another_systems"],
    ),
]

# ── main.rs: the command line ──────────────────────────────────────────────
MAIN = [
    (
        "--grub-add asks for an update",
        "        Some(\"--grub-add\") => grub(grubcmd::Verb::Add),",
        "        Some(\"--grub-add\") => grub(grubcmd::Verb::Update),",
        ["each_grub_flag_asks_for_its_own_command"],
    ),
    (
        "a path argument has to be text",
        "            .map(PathBuf::from)",
        "            .and_then(|a| a.to_str())\n            .map(PathBuf::from)",
        ["a_path_that_is_not_text_is_kept_as_it_is"],
    ),
    (
        "a GRUB command's options are dropped",
        "grubcmd::parse(verb, args.get(2..).unwrap_or_default())",
        "grubcmd::parse(verb, &[])",
        ["a_grub_commands_options_reach_it"],
    ),
]

TABLES = {
    "lib.rs": LIB,
    "grub.rs": GRUB,
    "grubcmd.rs": CMD,
    "main.rs": MAIN,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "installer", timeout=600, only=mine))
    raise SystemExit(worst)
