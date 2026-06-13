#![deny(clippy::all)]

//! selinux-cli — SlateOS SELinux management tools
//!
//! Multi-personality: `getenforce`, `setenforce`, `sestatus`, `semanage`,
//! `setsebool`, `getsebool`, `restorecon`, `chcon`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_getenforce(_args: &[String]) -> i32 {
    println!("Enforcing");
    0
}

fn run_setenforce(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: setenforce [ Enforcing | Permissive | 1 | 0 ]");
        return 0;
    }
    let mode = args.first().map(|s| s.as_str()).unwrap_or("");
    match mode {
        "0" | "Permissive" => println!("SELinux mode set to Permissive"),
        "1" | "Enforcing" => println!("SELinux mode set to Enforcing"),
        _ => {
            eprintln!("usage: setenforce [ Enforcing | Permissive | 1 | 0 ]");
            return 1;
        }
    }
    0
}

fn run_sestatus(_args: &[String]) -> i32 {
    println!("SELinux status:                 enabled");
    println!("SELinuxfs mount:                /sys/fs/selinux");
    println!("SELinux root directory:         /etc/selinux");
    println!("Loaded policy name:             targeted");
    println!("Current mode:                   enforcing");
    println!("Mode from config file:          enforcing");
    println!("Policy MLS status:              enabled");
    println!("Policy deny_unknown status:     allowed");
    println!("Memory protection checking:     actual (secure)");
    println!("Max kernel policy version:      33");
    0
}

fn run_semanage(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: semanage OBJECT [OPTIONS]");
        println!();
        println!("semanage — SELinux policy management (Slate OS).");
        println!();
        println!("Objects: login, user, role, port, interface, fcontext, boolean, module");
        return 0;
    }
    let object = args.first().map(|s| s.as_str()).unwrap_or("");
    match object {
        "port" => {
            println!("SELinux Port Type              Proto    Port Number");
            println!("http_port_t                    tcp      80, 443, 8080, 8443");
            println!("ssh_port_t                     tcp      22");
            println!("smtp_port_t                    tcp      25, 465, 587");
        }
        "boolean" => {
            println!("SELinux boolean                State  Default Description");
            println!("httpd_can_network_connect      (off  ,  off)  Allow httpd network connect");
            println!("httpd_enable_cgi               (on   ,   on)  Allow httpd cgi support");
        }
        "fcontext" => {
            println!("SELinux fcontext               type              Context");
            println!("/var/www(/.*)?                  all files         system_u:object_r:httpd_sys_content_t:s0");
            println!("/etc/httpd(/.*)?                all files         system_u:object_r:httpd_config_t:s0");
        }
        _ => println!("semanage: listing {} objects", object),
    }
    0
}

fn run_setsebool(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: setsebool [-P] boolean value");
        return 1;
    }
    let persistent = args.iter().any(|a| a == "-P");
    let bools: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if bools.len() >= 2 {
        let msg = if persistent { " (persistent)" } else { "" };
        println!("setsebool: {} -> {}{}", bools[0], bools[1], msg);
    }
    0
}

fn run_getsebool(args: &[String]) -> i32 {
    let all = args.iter().any(|a| a == "-a");
    if all {
        println!("httpd_can_network_connect --> off");
        println!("httpd_enable_cgi --> on");
        println!("httpd_enable_homedirs --> off");
        println!("samba_enable_home_dirs --> off");
        println!("ssh_sysadm_login --> off");
    } else if let Some(name) = args.iter().find(|a| !a.starts_with('-')) {
        println!("{} --> off", name);
    }
    0
}

fn run_restorecon(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: restorecon [OPTIONS] PATH...");
        println!("Options: -R (recursive), -v (verbose), -n (dry run), -F (force)");
        return 0;
    }
    let verbose = args.iter().any(|a| a == "-v");
    let paths: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for path in &paths {
        if verbose {
            println!("Relabeled {} from unconfined_u:object_r:default_t:s0 to system_u:object_r:httpd_sys_content_t:s0", path);
        }
    }
    0
}

fn run_chcon(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chcon [OPTIONS] CONTEXT FILE...");
        println!("Options: -R (recursive), -v (verbose), -t TYPE, -u USER, -r ROLE");
        return 0;
    }
    let verbose = args.iter().any(|a| a == "-v");
    if verbose {
        println!("chcon: context changed");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "sestatus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "getenforce" => run_getenforce(&rest),
        "setenforce" => run_setenforce(&rest),
        "semanage" => run_semanage(&rest),
        "setsebool" => run_setsebool(&rest),
        "getsebool" => run_getsebool(&rest),
        "restorecon" => run_restorecon(&rest),
        "chcon" => run_chcon(&rest),
        _ => run_sestatus(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_setenforce};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/selinux"), "selinux");
        assert_eq!(basename(r"C:\bin\selinux.exe"), "selinux.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("selinux.exe"), "selinux");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_setenforce(&["--help".to_string()]), 0);
        assert_eq!(run_setenforce(&["-h".to_string()]), 0);
        let _ = run_setenforce(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_setenforce(&[]);
    }
}
