#![deny(clippy::all)]

//! borg — OurOS BorgBackup deduplicating backup tool
//!
//! Single personality: `borg`

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _BORG_CACHE: &str = "~/.cache/borg";
const _BORG_KEYS: &str = "~/.config/borg/keys";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct RepoInfo {
    location: String,
    _id: String,
    _encrypted: bool,
    _encryption_mode: String,
    archives: Vec<ArchiveInfo>,
}

#[derive(Clone, Debug)]
struct ArchiveInfo {
    name: String,
    _id: String,
    start: String,
    _duration_secs: u64,
    original_size: u64,
    compressed_size: u64,
    deduplicated_size: u64,
    _nfiles: u64,
}

fn sample_repo() -> RepoInfo {
    RepoInfo {
        location: "/backup/borg-repo".to_string(),
        _id: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
        _encrypted: true,
        _encryption_mode: "repokey-blake2".to_string(),
        archives: vec![
            ArchiveInfo {
                name: "home-2025-05-20".to_string(),
                _id: "abcd1234".to_string(),
                start: "Mon, 2025-05-20 02:00:00".to_string(),
                _duration_secs: 245,
                original_size: 52_428_800_000,
                compressed_size: 31_457_280_000,
                deduplicated_size: 2_147_483_648,
                _nfiles: 80191,
            },
            ArchiveInfo {
                name: "home-2025-05-21".to_string(),
                _id: "efgh5678".to_string(),
                start: "Tue, 2025-05-21 02:00:00".to_string(),
                _duration_secs: 180,
                original_size: 52_530_000_000,
                compressed_size: 31_518_000_000,
                deduplicated_size: 104_857_600,
                _nfiles: 80250,
            },
            ArchiveInfo {
                name: "home-2025-05-22".to_string(),
                _id: "ijkl9012".to_string(),
                start: "Thu, 2025-05-22 02:00:00".to_string(),
                _duration_secs: 190,
                original_size: 52_650_000_000,
                compressed_size: 31_590_000_000,
                deduplicated_size: 125_829_120,
                _nfiles: 80310,
            },
        ],
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 { format!("{:.2} GB", bytes as f64 / 1_000_000_000.0) }
    else if bytes >= 1_000_000 { format!("{:.2} MB", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.2} kB", bytes as f64 / 1_000.0) }
    else { format!("{} B", bytes) }
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_borg(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: borg <command> [options] [arguments]");
            println!();
            println!("BorgBackup — deduplicating archiver.");
            println!();
            println!("Commands:");
            println!("  init           Initialize a new repository");
            println!("  create         Create a new archive");
            println!("  extract        Extract archive contents");
            println!("  list           List repository or archive contents");
            println!("  info           Show archive/repo information");
            println!("  delete         Delete archives");
            println!("  prune          Prune archives per retention policy");
            println!("  compact        Compact repository segments");
            println!("  check          Verify repository integrity");
            println!("  mount          Mount archive as FUSE filesystem");
            println!("  diff           Show differences between archives");
            println!("  rename         Rename an archive");
            println!("  key            Manage repository keys");
            println!("  --version      Show version");
            0
        }
        "--version" | "-V" => { println!("borg 0.1.0 (OurOS)"); 0 }
        "init" => borg_init(&cmd_args),
        "create" => borg_create(&cmd_args),
        "list" => borg_list(&cmd_args),
        "info" => borg_info(&cmd_args),
        "delete" => borg_delete(&cmd_args),
        "prune" => borg_prune(&cmd_args),
        "compact" => { println!("borg compact: compacting segments (simulated)"); println!("Repository size: 2.10 GB → 2.05 GB"); 0 }
        "check" => borg_check(&cmd_args),
        "extract" => { println!("borg extract: extracting archive (simulated)"); 0 }
        "mount" => { println!("borg mount: FUSE mount not available (simulated)"); 0 }
        "diff" => borg_diff(&cmd_args),
        other => { eprintln!("borg: unknown command '{}'", other); 1 }
    }
}

fn borg_init(args: &[String]) -> i32 {
    let encryption = args.iter().position(|a| a == "-e" || a == "--encryption")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("repokey");
    let repo = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/backup/repo");

    println!("Initializing repository at {}", repo);
    println!("Encryption: {}", encryption);
    if encryption != "none" {
        println!("Enter new passphrase: ********");
        println!("Enter same passphrase again: ********");
    }
    println!("Repository initialized (simulated).");
    0
}

fn borg_create(args: &[String]) -> i32 {
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let stats = args.iter().any(|a| a == "-s" || a == "--stats");

    println!("Creating archive (simulated)");
    if verbose {
        println!("A /home/user/Documents/report.pdf");
        println!("A /home/user/Documents/spreadsheet.xlsx");
        println!("U /home/user/.bashrc");
    }

    if stats {
        println!("                       Original size      Compressed size    Deduplicated size");
        println!("This archive:               48.94 GB             29.36 GB            120.00 MB");
        println!("All archives:              157.61 GB             94.57 GB              2.38 GB");
        println!();
        println!("                       Unique chunks         Total chunks");
        println!("Chunk index:                    5432               123456");
    }
    println!("Archive created (simulated).");
    0
}

fn borg_list(args: &[String]) -> i32 {
    let repo = sample_repo();
    let specific = args.iter().find(|a| a.contains("::"));

    if let Some(archive_spec) = specific {
        println!("Listing contents of archive {} (simulated):", archive_spec);
        println!("drwxr-xr-x user user     0 Mon, 2025-05-20 home/user");
        println!("drwxr-xr-x user user     0 Mon, 2025-05-20 home/user/Documents");
        println!("-rw-r--r-- user user  2048 Mon, 2025-05-20 home/user/Documents/report.pdf");
        println!("-rw-r--r-- user user  4096 Mon, 2025-05-20 home/user/.bashrc");
    } else {
        for a in &repo.archives {
            println!("{:<30} {}", a.name, a.start);
        }
    }
    0
}

fn borg_info(args: &[String]) -> i32 {
    let repo = sample_repo();
    let _specific = args.iter().find(|a| a.contains("::"));

    println!("Repository ID: {}", repo._id);
    println!("Location: {}", repo.location);
    println!("Encrypted: {}", if repo._encrypted { "Yes" } else { "No" });
    println!("Encryption: {}", repo._encryption_mode);
    println!("Cache: {}", _BORG_CACHE);
    println!();
    for a in &repo.archives {
        println!("Archive name: {}", a.name);
        println!("Archive fingerprint: {}", a._id);
        println!("Time (start): {}", a.start);
        println!("Duration: {} seconds", a._duration_secs);
        println!("Number of files: {}", a._nfiles);
        println!("                       Original size      Compressed size    Deduplicated size");
        println!("This archive:          {:>14}     {:>14}     {:>14}",
            format_size(a.original_size), format_size(a.compressed_size), format_size(a.deduplicated_size));
        println!();
    }
    0
}

fn borg_delete(args: &[String]) -> i32 {
    let target = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("(none)");
    println!("Deleting archive {} (simulated)", target);
    0
}

fn borg_prune(args: &[String]) -> i32 {
    let keep_daily = args.iter().position(|a| a == "--keep-daily")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(7);
    let keep_weekly = args.iter().position(|a| a == "--keep-weekly")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(4);

    println!("Pruning archives (simulated):");
    println!("  Keeping {} daily, {} weekly", keep_daily, keep_weekly);
    println!("  Pruning: home-2025-05-15 (older than retention)");
    println!("  Keeping: home-2025-05-20, home-2025-05-21, home-2025-05-22");
    0
}

fn borg_check(args: &[String]) -> i32 {
    let repair = args.iter().any(|a| a == "--repair");
    println!("Starting repository check (simulated)");
    println!("  Verifying repository data integrity...");
    println!("  Verifying archive metadata...");
    if repair {
        println!("  Running in repair mode...");
    }
    println!("Repository check complete: no errors found.");
    0
}

fn borg_diff(_args: &[String]) -> i32 {
    println!("Differences between archives (simulated):");
    println!(" added       2.0 kB home/user/Documents/new_file.txt");
    println!(" removed     1.5 kB home/user/Downloads/old_file.zip");
    println!("[modified]  4.0 kB home/user/.bashrc");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_borg(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_repo() {
        let repo = sample_repo();
        assert_eq!(repo.archives.len(), 3);
        assert!(repo._encrypted);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert!(format_size(1_500_000).contains("MB"));
        assert!(format_size(2_000_000_000).contains("GB"));
    }

    #[test]
    fn test_dedup_ratio() {
        let repo = sample_repo();
        // Second archive should have much smaller dedup size
        assert!(repo.archives[1].deduplicated_size < repo.archives[1].original_size / 100);
    }
}
