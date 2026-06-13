#![deny(clippy::all)]

//! ansible-cli — Slate OS Ansible automation CLI
//!
//! Multi-personality: `ansible`, `ansible-playbook`, `ansible-galaxy`, `ansible-vault`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit(&['/', '\\'][..]).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name {
        "ansible-playbook" => "playbook",
        "ansible-galaxy" => "galaxy",
        "ansible-vault" => "vault",
        _ => "ansible",
    }
}

fn run_ansible(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ansible <HOST_PATTERN> -m <MODULE> -a <ARGS>");
        println!();
        println!("Run ad-hoc commands on remote hosts.");
        println!();
        println!("Options:");
        println!("  -m, --module-name <MOD>  Module name (default: command)");
        println!("  -a, --args <ARGS>        Module arguments");
        println!("  -i, --inventory <FILE>   Inventory file");
        println!("  -u, --user <USER>        Remote user");
        println!("  -b, --become             Become (sudo)");
        println!("  -k, --ask-pass           Ask for SSH password");
        println!("  -K, --ask-become-pass    Ask for sudo password");
        println!("  --list-hosts             List matched hosts");
        println!("  -v, --verbose            Verbose (-vvv for more)");
        return 0;
    }

    let host = args.first().map(|s| s.as_str()).unwrap_or("all");
    let module = args.windows(2)
        .find(|w| w[0] == "-m" || w[0] == "--module-name")
        .map(|w| w[1].as_str())
        .unwrap_or("ping");

    println!("{} | SUCCESS => {{", host);
    println!("    \"ansible_facts\": {{");
    println!("        \"discovered_interpreter_python\": \"/usr/bin/python3\"");
    println!("    }},");
    println!("    \"changed\": false,");
    if module == "ping" {
        println!("    \"ping\": \"pong\"");
    } else {
        println!("    \"rc\": 0,");
        println!("    \"stdout\": \"command output\"");
    }
    println!("}}");
    0
}

fn run_playbook(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ansible-playbook [OPTIONS] <PLAYBOOK>...");
        println!();
        println!("Run Ansible playbooks.");
        println!();
        println!("Options:");
        println!("  -i, --inventory <FILE>   Inventory");
        println!("  -e, --extra-vars <VARS>  Extra variables");
        println!("  --tags <TAGS>            Only run tagged tasks");
        println!("  --skip-tags <TAGS>       Skip tagged tasks");
        println!("  --check                  Dry run");
        println!("  --diff                   Show diffs");
        println!("  --limit <PATTERN>        Limit to hosts");
        return 0;
    }

    let playbook = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("site.yml");

    println!("PLAY [Deploy web application] ************************************");
    println!();
    println!("TASK [Gathering Facts] ********************************************");
    println!("ok: [web-1]");
    println!("ok: [web-2]");
    println!();
    println!("TASK [Install packages] *******************************************");
    println!("changed: [web-1]");
    println!("changed: [web-2]");
    println!();
    println!("TASK [Copy configuration] *****************************************");
    println!("changed: [web-1]");
    println!("changed: [web-2]");
    println!();
    println!("TASK [Start service] **********************************************");
    println!("ok: [web-1]");
    println!("ok: [web-2]");
    println!();
    println!("PLAY RECAP *******************************************************");
    println!("web-1                      : ok=4    changed=2    unreachable=0    failed=0");
    println!("web-2                      : ok=4    changed=2    unreachable=0    failed=0");
    println!();
    println!("  (playbook: {})", playbook);
    0
}

fn run_galaxy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ansible-galaxy <COMMAND>");
        println!();
        println!("Commands:");
        println!("  role install     Install roles");
        println!("  role init        Create role scaffold");
        println!("  role list        List installed roles");
        println!("  collection install  Install collections");
        println!("  collection list     List installed collections");
        return 0;
    }

    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub2 = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (sub, sub2) {
        ("role", "list") => {
            println!("- geerlingguy.docker, 6.1.0");
            println!("- geerlingguy.nginx, 3.2.0");
            println!("- geerlingguy.postgresql, 3.5.0");
        }
        ("role", "install") => {
            let role = args.get(2).map(|s| s.as_str()).unwrap_or("geerlingguy.docker");
            println!("- downloading role '{}' ...", role);
            println!("- {} was installed successfully", role);
        }
        ("collection", "list") => {
            println!("# /usr/share/ansible/collections");
            println!("Collection             Version");
            println!("────────────────────── ───────");
            println!("community.general      8.3.0");
            println!("ansible.posix          1.5.4");
            println!("community.docker       3.6.0");
        }
        _ => println!("Usage: ansible-galaxy <role|collection> <install|list|init>"),
    }
    0
}

fn run_vault(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ansible-vault <COMMAND> [OPTIONS] <FILE>");
        println!();
        println!("Commands:");
        println!("  encrypt        Encrypt a file");
        println!("  decrypt        Decrypt a file");
        println!("  edit           Edit encrypted file");
        println!("  view           View encrypted file");
        println!("  encrypt_string Encrypt a string");
        println!("  rekey          Re-encrypt with new password");
        return 0;
    }

    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "encrypt" => println!("Encryption successful"),
        "decrypt" => println!("Decryption successful"),
        "view" => {
            println!("db_password: supersecret123");
            println!("api_key: sk-abc123def456");
        }
        "rekey" => println!("Rekey successful"),
        _ => println!("Usage: ansible-vault <encrypt|decrypt|edit|view|rekey>"),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("ansible"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.iter().any(|a| a == "-V" || a == "--version") {
        println!("ansible [core 2.16.3] (Slate OS)");
        process::exit(0);
    }

    let code = match p {
        "ansible" => run_ansible(&rest),
        "playbook" => run_playbook(&rest),
        "galaxy" => run_galaxy(&rest),
        "vault" => run_vault(&rest),
        _ => run_ansible(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ansible};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ansible(&["--help".to_string()]), 0);
        assert_eq!(run_ansible(&["-h".to_string()]), 0);
        let _ = run_ansible(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ansible(&[]);
    }
}
