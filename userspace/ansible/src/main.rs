#![deny(clippy::all)]

//! ansible — SlateOS IT automation tool
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `ansible` (default) — run ad-hoc commands
//! - `ansible-playbook` — run playbooks
//! - `ansible-galaxy` — manage roles and collections
//! - `ansible-vault` — encrypt/decrypt data
//! - `ansible-inventory` — show inventory info
//! - `ansible-config` — show configuration
//! - `ansible-doc` — documentation viewer

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_ansible(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: ansible [-i INVENTORY] [-m MODULE] [-a ARGS] PATTERN");
        println!();
        println!("Run ad-hoc commands on remote hosts.");
        println!();
        println!("Options:");
        println!("  -i INVENTORY    Specify inventory host path");
        println!("  -m MODULE       Module name to execute (default=command)");
        println!("  -a ARGS         Module arguments");
        println!("  -u USER         Connect as this user");
        println!("  -b, --become    Run operations with become");
        println!("  -k              Ask for connection password");
        println!("  -f FORKS        Parallel processes (default=5)");
        println!("  -v              Verbose mode (-vvv for more)");
        println!("  --version       Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("ansible [core 2.16.0] (Slate OS)");
        println!("  python version = 3.13.0");
        return 0;
    }

    let module = args.iter().position(|a| a == "-m")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("command");
    let module_args = args.iter().position(|a| a == "-a")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("uptime");
    let pattern = args.iter().find(|a| !a.starts_with('-') && *a != module && *a != module_args)
        .map(|s| s.as_str())
        .unwrap_or("all");

    println!("{} | CHANGED | rc=0 >>", pattern);
    println!("(module={}, args=\"{}\" — simulated)", module, module_args);
    0
}

fn run_playbook(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: ansible-playbook [options] playbook.yml [playbook2 ...]");
        println!();
        println!("Options:");
        println!("  -i INVENTORY     Specify inventory");
        println!("  -l SUBSET        Limit to hosts");
        println!("  -t TAGS          Only run tagged tasks");
        println!("  --skip-tags TAGS Skip tagged tasks");
        println!("  -e EXTRA_VARS    Set additional variables");
        println!("  -C, --check      Don't make changes (dry run)");
        println!("  -D, --diff       Show file differences");
        println!("  -v               Verbose mode");
        println!("  --syntax-check   Perform a syntax check on the playbook");
        return 0;
    }

    let playbook = args.iter().find(|a| a.ends_with(".yml") || a.ends_with(".yaml"))
        .map(|s| s.as_str())
        .unwrap_or("site.yml");
    let check_mode = args.iter().any(|a| a == "-C" || a == "--check");

    println!("PLAY [all] ************************************************************");
    println!();
    println!("TASK [Gathering Facts] ************************************************");
    println!("ok: [web01]");
    println!("ok: [web02]");
    println!("ok: [db01]");
    println!();
    println!("TASK [Install packages] ***********************************************");
    if check_mode {
        println!("changed: [web01] (check mode)");
        println!("changed: [web02] (check mode)");
    } else {
        println!("changed: [web01]");
        println!("changed: [web02]");
    }
    println!("ok: [db01]");
    println!();
    println!("TASK [Start service] **************************************************");
    println!("ok: [web01]");
    println!("ok: [web02]");
    println!("ok: [db01]");
    println!();
    println!("PLAY RECAP *************************************************************");
    println!("web01                      : ok=3    changed=1    unreachable=0    failed=0    skipped=0");
    println!("web02                      : ok=3    changed=1    unreachable=0    failed=0    skipped=0");
    println!("db01                       : ok=3    changed=0    unreachable=0    failed=0    skipped=0");
    println!();
    println!("(playbook: {} — simulated)", playbook);
    0
}

fn run_galaxy(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("usage: ansible-galaxy <type> <command> [options]");
            println!();
            println!("Types: role, collection");
            println!();
            println!("Commands:");
            println!("  init       Create initial role/collection structure");
            println!("  install    Install role/collection");
            println!("  list       List installed roles/collections");
            println!("  search     Search Galaxy for roles");
            println!("  remove     Remove role/collection");
            0
        }
        "role" | "collection" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "init" => {
                    let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("myrole");
                    println!("- Role {} was created successfully", name);
                }
                "install" => {
                    let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("role");
                    println!("- downloading role '{}' ...", name);
                    println!("- extracting {} to /home/user/.ansible/roles/{}", name, name);
                    println!("- {} was installed successfully", name);
                }
                "list" => {
                    println!("- geerlingguy.docker, 6.2.0");
                    println!("- geerlingguy.nginx, 3.2.0");
                    println!("- geerlingguy.postgresql, 3.5.0");
                }
                _ => println!("{} {}: (simulated)", cmd, sub),
            }
            0
        }
        other => { eprintln!("ansible-galaxy: unknown type '{}'", other); 1 }
    }
}

fn run_vault(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("usage: ansible-vault <command> [options] [args]");
            println!();
            println!("Commands:");
            println!("  create     Create new encrypted file");
            println!("  decrypt    Decrypt an encrypted file");
            println!("  edit       Edit an encrypted file");
            println!("  encrypt    Encrypt a file");
            println!("  encrypt_string  Encrypt a string");
            println!("  rekey      Re-key an encrypted file");
            println!("  view       View an encrypted file");
            0
        }
        "encrypt" => { println!("Encryption successful (simulated)"); 0 }
        "decrypt" => { println!("Decryption successful (simulated)"); 0 }
        "view" => { println!("(viewing encrypted file — simulated)"); 0 }
        "create" => { println!("(creating encrypted file — simulated)"); 0 }
        "edit" => { println!("(editing encrypted file — simulated)"); 0 }
        "rekey" => { println!("Rekey successful (simulated)"); 0 }
        "encrypt_string" => { println!("!vault |\n  $ANSIBLE_VAULT;1.1;AES256\n  (encrypted data — simulated)"); 0 }
        other => { eprintln!("ansible-vault: unknown command '{}'", other); 1 }
    }
}

fn run_inventory(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: ansible-inventory [options] [host|group]");
        return 0;
    }
    let list = args.iter().any(|a| a == "--list");
    let graph = args.iter().any(|a| a == "--graph");

    if graph {
        println!("@all:");
        println!("  |--@webservers:");
        println!("  |  |--web01");
        println!("  |  |--web02");
        println!("  |--@dbservers:");
        println!("  |  |--db01");
    } else if list {
        println!("{{");
        println!("  \"webservers\": {{\"hosts\": [\"web01\", \"web02\"]}},");
        println!("  \"dbservers\": {{\"hosts\": [\"db01\"]}}");
        println!("}}");
    } else {
        println!("web01, web02, db01");
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("ansible");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "ansible-playbook" => run_playbook(rest),
        "ansible-galaxy" => run_galaxy(rest),
        "ansible-vault" => run_vault(rest),
        "ansible-inventory" => run_inventory(rest),
        "ansible-config" => { println!("(ansible-config — simulated)"); 0 }
        "ansible-doc" => { println!("(ansible-doc — simulated)"); 0 }
        _ => run_ansible(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_ansible};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ansible(vec!["--help".to_string()]), 0);
        assert_eq!(run_ansible(vec!["-h".to_string()]), 0);
        let _ = run_ansible(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ansible(vec![]);
    }
}
