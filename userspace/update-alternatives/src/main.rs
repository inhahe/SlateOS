//! SlateOS alternatives management system.
//!
//! Multi-personality binary providing:
//! - **update-alternatives** — maintain symlinks for default commands
//! - **alternatives** — RedHat-compatible alias
//!
//! Manages a system of symlinks that allow multiple versions of the same
//! program to coexist, with a central registry tracking which version is
//! the "default" for each command.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

/// Represents a single alternative choice for a command group.
#[derive(Clone, Debug, PartialEq)]
struct Alternative {
    /// Path to the actual binary.
    path: String,
    /// Priority (higher = preferred in auto mode).
    priority: i32,
    /// Slave links: (slave_name, slave_path).
    slaves: Vec<(String, String)>,
}

/// A command group (e.g., "editor", "cc", "java").
#[derive(Clone, Debug)]
struct AlternativeGroup {
    /// Name of the group (e.g., "editor").
    name: String,
    /// The generic link path (e.g., "/usr/bin/editor").
    link: String,
    /// Slave link definitions: (slave_name, slave_link_path).
    slave_links: Vec<(String, String)>,
    /// Available alternatives.
    alternatives: Vec<Alternative>,
    /// Current mode: "auto" or "manual".
    mode: String,
    /// Currently selected alternative path (manual mode) or empty (auto).
    current: String,
}

impl AlternativeGroup {
    fn best_alternative(&self) -> Option<&Alternative> {
        self.alternatives.iter().max_by_key(|a| a.priority)
    }

    fn current_alternative(&self) -> Option<&Alternative> {
        if self.mode == "auto" {
            self.best_alternative()
        } else {
            self.alternatives.iter().find(|a| a.path == self.current)
        }
    }

    fn current_path(&self) -> String {
        if let Some(alt) = self.current_alternative() {
            alt.path.clone()
        } else if let Some(alt) = self.best_alternative() {
            alt.path.clone()
        } else {
            String::new()
        }
    }
}

// ============================================================================
// Registry (persistent storage)
// ============================================================================

const ADMIN_DIR: &str = "/var/lib/alternatives";
const ALT_DIR: &str = "/etc/alternatives";

fn load_group(name: &str) -> Option<AlternativeGroup> {
    let path = Path::new(ADMIN_DIR).join(name);
    let content = fs::read_to_string(&path).ok()?;
    parse_group_file(name, &content)
}

fn save_group(group: &AlternativeGroup) -> io::Result<()> {
    let admin_dir = Path::new(ADMIN_DIR);
    fs::create_dir_all(admin_dir)?;

    let path = admin_dir.join(&group.name);
    let content = serialize_group(group);
    fs::write(&path, content)?;

    // Update symlinks.
    update_symlinks(group)?;

    Ok(())
}

fn remove_group_file(name: &str) -> io::Result<()> {
    let path = Path::new(ADMIN_DIR).join(name);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

fn list_groups() -> Vec<String> {
    let mut groups = Vec::new();
    if let Ok(entries) = fs::read_dir(ADMIN_DIR) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && !name.starts_with('.')
            {
                groups.push(name.to_string());
            }
        }
    }
    groups.sort();
    groups
}

// ============================================================================
// File format (Debian-style)
// ============================================================================

fn parse_group_file(name: &str, content: &str) -> Option<AlternativeGroup> {
    let mut lines = content.lines();

    let mode = lines.next()?.trim().to_string();
    let link = lines.next()?.trim().to_string();

    // Read slave link definitions.
    let mut slave_links = Vec::new();
    loop {
        let line = lines.next()?.trim().to_string();
        if line.is_empty() {
            break;
        }
        if let Some((sname, spath)) = line.split_once(' ') {
            slave_links.push((sname.to_string(), spath.to_string()));
        }
    }

    // Read alternatives.
    let mut alternatives = Vec::new();
    while let Some(line) = lines.next() {
        let path = line.trim().to_string();
        if path.is_empty() {
            continue;
        }
        let priority: i32 = lines
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0);

        // Read slave paths.
        let mut slaves = Vec::new();
        for sl in &slave_links {
            if let Some(slave_path_line) = lines.next() {
                let sp = slave_path_line.trim().to_string();
                if !sp.is_empty() {
                    slaves.push((sl.0.clone(), sp));
                }
            }
        }

        alternatives.push(Alternative {
            path,
            priority,
            slaves,
        });
    }

    let current = if mode == "manual" {
        alternatives
            .first()
            .map(|a| a.path.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };

    Some(AlternativeGroup {
        name: name.to_string(),
        link,
        slave_links,
        alternatives,
        mode,
        current,
    })
}

fn serialize_group(group: &AlternativeGroup) -> String {
    let mut out = String::new();
    out.push_str(&group.mode);
    out.push('\n');
    out.push_str(&group.link);
    out.push('\n');

    for (sname, spath) in &group.slave_links {
        out.push_str(&format!("{sname} {spath}\n"));
    }
    out.push('\n');

    for alt in &group.alternatives {
        out.push_str(&alt.path);
        out.push('\n');
        out.push_str(&format!("{}\n", alt.priority));
        for sl in &group.slave_links {
            let slave_path = alt
                .slaves
                .iter()
                .find(|(n, _)| n == &sl.0)
                .map(|(_, p)| p.as_str())
                .unwrap_or("");
            out.push_str(slave_path);
            out.push('\n');
        }
    }

    out
}

fn update_symlinks(group: &AlternativeGroup) -> io::Result<()> {
    let alt_dir = Path::new(ALT_DIR);
    fs::create_dir_all(alt_dir)?;

    let target = group.current_path();
    if target.is_empty() {
        // Remove the symlink if no alternatives remain.
        let link_path = alt_dir.join(&group.name);
        if link_path.exists() {
            let _ = fs::remove_file(&link_path);
        }
        return Ok(());
    }

    // Create/update the alternatives symlink.
    let alt_link = alt_dir.join(&group.name);
    let _ = fs::remove_file(&alt_link);
    // In a real OS, create a symlink. Here we write a marker file.
    fs::write(&alt_link, format!("-> {target}\n"))?;

    // Update the main link.
    let main_link = PathBuf::from(&group.link);
    if let Some(parent) = main_link.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(&main_link);
    fs::write(&main_link, format!("-> {}\n", alt_link.display()))?;

    Ok(())
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_install(
    link: &str,
    name: &str,
    path: &str,
    priority: i32,
    slaves: &[(String, String, String)],
) -> i32 {
    let mut group = load_group(name).unwrap_or_else(|| AlternativeGroup {
        name: name.to_string(),
        link: link.to_string(),
        slave_links: Vec::new(),
        alternatives: Vec::new(),
        mode: "auto".to_string(),
        current: String::new(),
    });

    // Update link if different.
    if group.link != link {
        group.link = link.to_string();
    }

    // Add/update slave link definitions.
    for (sname, slink, _) in slaves {
        if !group.slave_links.iter().any(|(n, _)| n == sname) {
            group.slave_links.push((sname.clone(), slink.clone()));
        }
    }

    // Build slave paths for this alternative.
    let slave_entries: Vec<(String, String)> = slaves
        .iter()
        .map(|(sname, _, spath)| (sname.clone(), spath.clone()))
        .collect();

    // Remove existing alternative with same path.
    group.alternatives.retain(|a| a.path != path);

    // Add new alternative.
    group.alternatives.push(Alternative {
        path: path.to_string(),
        priority,
        slaves: slave_entries,
    });

    match save_group(&group) {
        Ok(()) => {
            println!("update-alternatives: using {path} to provide {link} ({name}) in auto mode");
            0
        }
        Err(e) => {
            eprintln!("update-alternatives: error: {e}");
            1
        }
    }
}

fn cmd_remove(name: &str, path: &str) -> i32 {
    let mut group = match load_group(name) {
        Some(g) => g,
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            return 1;
        }
    };

    let before = group.alternatives.len();
    group.alternatives.retain(|a| a.path != path);

    if group.alternatives.len() == before {
        eprintln!("update-alternatives: alternative {path} not found for {name}");
        return 1;
    }

    if group.alternatives.is_empty() {
        let _ = remove_group_file(name);
        println!("update-alternatives: removing {name} entirely, no alternatives left");
    } else {
        if let Err(e) = save_group(&group) {
            eprintln!("update-alternatives: error: {e}");
            return 1;
        }
    }

    0
}

fn cmd_set(name: &str, path: &str) -> i32 {
    let mut group = match load_group(name) {
        Some(g) => g,
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            return 1;
        }
    };

    if !group.alternatives.iter().any(|a| a.path == path) {
        eprintln!("update-alternatives: alternative {path} not found for {name}");
        return 1;
    }

    group.mode = "manual".to_string();
    group.current = path.to_string();

    match save_group(&group) {
        Ok(()) => {
            println!("update-alternatives: set {name} to {path} (manual mode)");
            0
        }
        Err(e) => {
            eprintln!("update-alternatives: error: {e}");
            1
        }
    }
}

fn cmd_auto(name: &str) -> i32 {
    let mut group = match load_group(name) {
        Some(g) => g,
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            return 1;
        }
    };

    group.mode = "auto".to_string();
    group.current.clear();

    match save_group(&group) {
        Ok(()) => {
            if let Some(best) = group.best_alternative() {
                println!(
                    "update-alternatives: {name} set to auto mode, currently {}",
                    best.path
                );
            }
            0
        }
        Err(e) => {
            eprintln!("update-alternatives: error: {e}");
            1
        }
    }
}

fn cmd_display(name: &str) -> i32 {
    let group = match load_group(name) {
        Some(g) => g,
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            return 1;
        }
    };

    println!("{} - {} mode", group.name, group.mode);
    println!("  link currently points to {}", group.current_path());
    println!("  link {} is {}", group.name, group.link);

    for (sname, slink) in &group.slave_links {
        println!("  slave {sname} is {slink}");
    }

    for alt in &group.alternatives {
        println!("{} - priority {}", alt.path, alt.priority);
        for (sname, spath) in &alt.slaves {
            println!("  slave {sname}: {spath}");
        }
    }

    0
}

fn cmd_query(name: &str) -> i32 {
    let group = match load_group(name) {
        Some(g) => g,
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            return 1;
        }
    };

    println!("Name: {}", group.name);
    println!("Link: {}", group.link);
    println!("Status: {}", group.mode);
    println!(
        "Best: {}",
        group
            .best_alternative()
            .map(|a| a.path.as_str())
            .unwrap_or("none")
    );
    println!("Value: {}", group.current_path());
    println!();

    for alt in &group.alternatives {
        println!("Alternative: {}", alt.path);
        println!("Priority: {}", alt.priority);
        for (sname, spath) in &alt.slaves {
            println!("Slave-{sname}: {spath}");
        }
        println!();
    }

    0
}

fn cmd_list(name: &str) -> i32 {
    let group = match load_group(name) {
        Some(g) => g,
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            return 1;
        }
    };

    for alt in &group.alternatives {
        println!("{}", alt.path);
    }

    0
}

fn cmd_get_selections() -> i32 {
    let groups = list_groups();
    for name in &groups {
        if let Some(group) = load_group(name) {
            println!(
                "{:<30} {:>8} {}",
                group.name,
                group.mode,
                group.current_path()
            );
        }
    }
    0
}

fn cmd_config(name: &str) -> i32 {
    let mut group = match load_group(name) {
        Some(g) => g,
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            return 1;
        }
    };

    if group.alternatives.is_empty() {
        eprintln!("update-alternatives: no alternatives for {name}");
        return 1;
    }

    println!(
        "There are {} choices for {}:",
        group.alternatives.len(),
        group.name
    );
    println!();
    println!("  {:>4}  {:<50} {:>8}", "Sel", "Path", "Priority");
    println!("{}", "-".repeat(70));

    let current = group.current_path();
    for (idx, alt) in group.alternatives.iter().enumerate() {
        let marker = if alt.path == current { "*" } else { " " };
        println!("{marker} {:>4}  {:<50} {:>8}", idx, alt.path, alt.priority);
    }
    println!();

    eprint!("Press <enter> to keep the current choice[*], or type selection number: ");
    let _ = io::stderr().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    let input = input.trim();

    if input.is_empty() {
        println!("Nothing to configure.");
        return 0;
    }

    if let Ok(idx) = input.parse::<usize>() {
        if idx < group.alternatives.len() {
            group.mode = "manual".to_string();
            group.current = group.alternatives[idx].path.clone();
            if let Err(e) = save_group(&group) {
                eprintln!("update-alternatives: error: {e}");
                return 1;
            }
            println!("update-alternatives: set {name} to {}", group.current);
        } else {
            eprintln!("update-alternatives: invalid selection {idx}");
            return 1;
        }
    }

    0
}

fn cmd_remove_all(name: &str) -> i32 {
    match load_group(name) {
        Some(_) => {
            if let Err(e) = remove_group_file(name) {
                eprintln!("update-alternatives: error: {e}");
                return 1;
            }
            println!("update-alternatives: removed all alternatives for {name}");
            0
        }
        None => {
            eprintln!("update-alternatives: no alternatives for {name}");
            1
        }
    }
}

// ============================================================================
// Main entry points
// ============================================================================

fn update_alternatives_main(args: &[String]) -> i32 {
    if args.is_empty() {
        print_help();
        return 1;
    }

    let mut i = 0;
    let mut _alt_dir = ALT_DIR.to_string();
    let mut _admin_dir = ADMIN_DIR.to_string();
    let mut verbose = false;

    // Parse global options first.
    while i < args.len() {
        match args[i].as_str() {
            "--altdir" => {
                i += 1;
                if i < args.len() {
                    _alt_dir = args[i].clone();
                }
            }
            "--admindir" => {
                i += 1;
                if i < args.len() {
                    _admin_dir = args[i].clone();
                }
            }
            "--verbose" => verbose = true,
            "--quiet" => {} // Ignored, always quiet.
            _ => break,
        }
        i += 1;
    }

    if i >= args.len() {
        print_help();
        return 1;
    }

    let command = args[i].as_str();
    let cmd_args = &args[i + 1..];

    match command {
        "--install" => {
            if cmd_args.len() < 4 {
                eprintln!("update-alternatives: --install needs <link> <name> <path> <priority>");
                return 1;
            }
            let link = &cmd_args[0];
            let name = &cmd_args[1];
            let path = &cmd_args[2];
            let priority: i32 = cmd_args[3].parse().unwrap_or(0);

            // Parse optional --slave triplets.
            let mut slaves = Vec::new();
            let mut j = 4;
            while j + 3 < cmd_args.len() && cmd_args[j] == "--slave" {
                slaves.push((
                    cmd_args[j + 2].clone(), // slave name
                    cmd_args[j + 1].clone(), // slave link
                    cmd_args[j + 3].clone(), // slave path
                ));
                j += 4;
            }

            if verbose {
                eprintln!("update-alternatives: installing {path} as {name} (priority {priority})");
            }

            cmd_install(link, name, path, priority, &slaves)
        }
        "--remove" => {
            if cmd_args.len() < 2 {
                eprintln!("update-alternatives: --remove needs <name> <path>");
                return 1;
            }
            cmd_remove(&cmd_args[0], &cmd_args[1])
        }
        "--set" => {
            if cmd_args.len() < 2 {
                eprintln!("update-alternatives: --set needs <name> <path>");
                return 1;
            }
            cmd_set(&cmd_args[0], &cmd_args[1])
        }
        "--auto" => {
            if cmd_args.is_empty() {
                eprintln!("update-alternatives: --auto needs <name>");
                return 1;
            }
            cmd_auto(&cmd_args[0])
        }
        "--display" => {
            if cmd_args.is_empty() {
                eprintln!("update-alternatives: --display needs <name>");
                return 1;
            }
            cmd_display(&cmd_args[0])
        }
        "--query" => {
            if cmd_args.is_empty() {
                eprintln!("update-alternatives: --query needs <name>");
                return 1;
            }
            cmd_query(&cmd_args[0])
        }
        "--list" => {
            if cmd_args.is_empty() {
                eprintln!("update-alternatives: --list needs <name>");
                return 1;
            }
            cmd_list(&cmd_args[0])
        }
        "--config" => {
            if cmd_args.is_empty() {
                eprintln!("update-alternatives: --config needs <name>");
                return 1;
            }
            cmd_config(&cmd_args[0])
        }
        "--get-selections" => cmd_get_selections(),
        "--remove-all" => {
            if cmd_args.is_empty() {
                eprintln!("update-alternatives: --remove-all needs <name>");
                return 1;
            }
            cmd_remove_all(&cmd_args[0])
        }
        "--help" | "-h" => {
            print_help();
            0
        }
        "--version" => {
            println!("update-alternatives (SlateOS) {VERSION}");
            0
        }
        other => {
            eprintln!("update-alternatives: unknown command '{other}'");
            1
        }
    }
}

fn print_help() {
    println!("Usage: update-alternatives [options] <command>");
    println!();
    println!("Commands:");
    println!("  --install <link> <name> <path> <prio> [--slave ...]");
    println!("  --remove <name> <path>");
    println!("  --remove-all <name>");
    println!("  --set <name> <path>");
    println!("  --auto <name>");
    println!("  --display <name>");
    println!("  --query <name>");
    println!("  --list <name>");
    println!("  --config <name>");
    println!("  --get-selections");
    println!();
    println!("Options:");
    println!("  --altdir <dir>    Alternatives directory");
    println!("  --admindir <dir>  Admin directory");
    println!("  --verbose         Verbose output");
    println!("  --help            Display this help");
    println!("  --version         Display version");
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    // `update-alternatives` and its `alternatives` alias share a single entry
    // point, so there is no need to dispatch on the invocation name.
    let exit_code = update_alternatives_main(&rest);

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_group() -> AlternativeGroup {
        AlternativeGroup {
            name: "editor".to_string(),
            link: "/usr/bin/editor".to_string(),
            slave_links: vec![(
                "editor.1".to_string(),
                "/usr/share/man/man1/editor.1".to_string(),
            )],
            alternatives: vec![
                Alternative {
                    path: "/usr/bin/vim".to_string(),
                    priority: 50,
                    slaves: vec![(
                        "editor.1".to_string(),
                        "/usr/share/man/man1/vim.1".to_string(),
                    )],
                },
                Alternative {
                    path: "/usr/bin/nano".to_string(),
                    priority: 40,
                    slaves: vec![(
                        "editor.1".to_string(),
                        "/usr/share/man/man1/nano.1".to_string(),
                    )],
                },
                Alternative {
                    path: "/usr/bin/emacs".to_string(),
                    priority: 60,
                    slaves: vec![(
                        "editor.1".to_string(),
                        "/usr/share/man/man1/emacs.1".to_string(),
                    )],
                },
            ],
            mode: "auto".to_string(),
            current: String::new(),
        }
    }

    #[test]
    fn test_best_alternative() {
        let group = sample_group();
        let best = group.best_alternative().unwrap();
        assert_eq!(best.path, "/usr/bin/emacs");
        assert_eq!(best.priority, 60);
    }

    #[test]
    fn test_current_alternative_auto() {
        let group = sample_group();
        let current = group.current_alternative().unwrap();
        assert_eq!(current.path, "/usr/bin/emacs"); // Highest priority.
    }

    #[test]
    fn test_current_alternative_manual() {
        let mut group = sample_group();
        group.mode = "manual".to_string();
        group.current = "/usr/bin/nano".to_string();
        let current = group.current_alternative().unwrap();
        assert_eq!(current.path, "/usr/bin/nano");
    }

    #[test]
    fn test_current_path_auto() {
        let group = sample_group();
        assert_eq!(group.current_path(), "/usr/bin/emacs");
    }

    #[test]
    fn test_current_path_manual() {
        let mut group = sample_group();
        group.mode = "manual".to_string();
        group.current = "/usr/bin/vim".to_string();
        assert_eq!(group.current_path(), "/usr/bin/vim");
    }

    #[test]
    fn test_serialize_parse_roundtrip() {
        let group = sample_group();
        let serialized = serialize_group(&group);
        let parsed = parse_group_file("editor", &serialized).unwrap();

        assert_eq!(parsed.name, group.name);
        assert_eq!(parsed.link, group.link);
        assert_eq!(parsed.mode, group.mode);
        assert_eq!(parsed.alternatives.len(), group.alternatives.len());

        for (orig, parsed) in group.alternatives.iter().zip(parsed.alternatives.iter()) {
            assert_eq!(orig.path, parsed.path);
            assert_eq!(orig.priority, parsed.priority);
        }
    }

    #[test]
    fn test_empty_group() {
        let group = AlternativeGroup {
            name: "test".to_string(),
            link: "/usr/bin/test".to_string(),
            slave_links: Vec::new(),
            alternatives: Vec::new(),
            mode: "auto".to_string(),
            current: String::new(),
        };
        assert!(group.best_alternative().is_none());
        assert!(group.current_alternative().is_none());
        assert!(group.current_path().is_empty());
    }

    #[test]
    fn test_single_alternative() {
        let group = AlternativeGroup {
            name: "cc".to_string(),
            link: "/usr/bin/cc".to_string(),
            slave_links: Vec::new(),
            alternatives: vec![Alternative {
                path: "/usr/bin/gcc".to_string(),
                priority: 20,
                slaves: Vec::new(),
            }],
            mode: "auto".to_string(),
            current: String::new(),
        };
        assert_eq!(group.current_path(), "/usr/bin/gcc");
    }

    #[test]
    fn test_list_groups_nonexistent_dir() {
        // Should return empty, not crash.
        let _ = list_groups();
    }

    #[test]
    fn test_load_group_nonexistent() {
        assert!(load_group("nonexistent_test_group_xyz").is_none());
    }

    #[test]
    fn test_alternative_equality() {
        let a = Alternative {
            path: "/usr/bin/vim".to_string(),
            priority: 50,
            slaves: Vec::new(),
        };
        let b = Alternative {
            path: "/usr/bin/vim".to_string(),
            priority: 50,
            slaves: Vec::new(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_alternative_inequality() {
        let a = Alternative {
            path: "/usr/bin/vim".to_string(),
            priority: 50,
            slaves: Vec::new(),
        };
        let b = Alternative {
            path: "/usr/bin/nano".to_string(),
            priority: 40,
            slaves: Vec::new(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn test_serialize_no_slaves() {
        let group = AlternativeGroup {
            name: "test".to_string(),
            link: "/usr/bin/test".to_string(),
            slave_links: Vec::new(),
            alternatives: vec![Alternative {
                path: "/usr/bin/test-impl".to_string(),
                priority: 10,
                slaves: Vec::new(),
            }],
            mode: "auto".to_string(),
            current: String::new(),
        };
        let serialized = serialize_group(&group);
        assert!(serialized.contains("auto"));
        assert!(serialized.contains("/usr/bin/test-impl"));
    }
}
