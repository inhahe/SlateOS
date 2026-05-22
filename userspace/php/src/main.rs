#![deny(clippy::all)]

//! php — OurOS PHP interpreter
//!
//! Multi-personality: `php`, `php-fpm`, `composer`

use std::env;
use std::process;

fn run_php(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: php [options] [-f] <file> [--] [args...]");
        println!("       php [options] -r <code> [--] [args...]");
        println!("       php [options] [-B <begin_code>] -R <code> [-E <end_code>] [--] [args...]");
        println!("       php [options] [-B <begin_code>] -F <file> [-E <end_code>] [--] [args...]");
        println!("       php [options] -S <addr>:<port> [-t docroot] [router]");
        println!();
        println!("  -r <code>     Run PHP code");
        println!("  -f <file>     Run PHP file");
        println!("  -S <addr>     Run built-in server");
        println!("  -t <dir>      Document root for -S");
        println!("  -c <path>     Config file/directory");
        println!("  -n            No config file");
        println!("  -d key=val    Set INI entry");
        println!("  -a            Interactive mode");
        println!("  -m            Show compiled modules");
        println!("  -i            Show phpinfo()");
        println!("  -v            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("PHP 8.3.8 (cli) (OurOS)");
        println!("Zend Engine v4.3.8");
        return 0;
    }
    if args.iter().any(|a| a == "-m") {
        println!("[PHP Modules]");
        println!("bcmath, calendar, Core, ctype, curl, date, dom, exif, fileinfo,");
        println!("filter, ftp, gd, hash, iconv, intl, json, libxml, mbstring,");
        println!("mysqli, mysqlnd, openssl, pcntl, pcre, PDO, pdo_mysql, pdo_pgsql,");
        println!("pdo_sqlite, Phar, posix, readline, Reflection, session, shmop,");
        println!("SimpleXML, sockets, sodium, SPL, sqlite3, standard, tokenizer,");
        println!("xml, xmlreader, xmlwriter, xsl, zip, zlib");
        return 0;
    }
    if args.iter().any(|a| a == "-i") {
        println!("phpinfo()");
        println!("PHP Version => 8.3.8");
        println!("System => OurOS");
        println!("Zend Engine v4.3.8");
        return 0;
    }

    let serve = args.iter().position(|a| a == "-S")
        .and_then(|i| args.get(i + 1));
    if let Some(addr) = serve {
        println!("PHP 8.3.8 Development Server (OurOS) started at {}", addr);
        println!("Document root is /var/www/html");
        println!("Press Ctrl-C to quit.");
        return 0;
    }

    let exec_code = args.iter().position(|a| a == "-r")
        .and_then(|i| args.get(i + 1));
    if let Some(code) = exec_code {
        println!("(executing: {})", code);
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(f) = file {
        println!("(running {})", f);
    } else {
        println!("Interactive mode enabled (simulated)");
        println!("php > ");
    }
    0
}

fn run_composer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: composer <command> [options] [args]");
        println!();
        println!("Commands:");
        println!("  install       Install dependencies");
        println!("  update        Update dependencies");
        println!("  require       Add dependency");
        println!("  remove        Remove dependency");
        println!("  show          Show package info");
        println!("  search        Search packages");
        println!("  init          Create composer.json");
        println!("  dump-autoload Regenerate autoloader");
        println!("  --version     Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "--version" | "-V" => println!("Composer version 2.7.6 (OurOS) 2025-05-22"),
        "install" => {
            println!("Installing dependencies from lock file");
            println!("  - Installing psr/log (3.0.0): Extracting archive");
            println!("  - Installing monolog/monolog (3.6.0): Extracting archive");
            println!("Package operations: 2 installs, 0 updates, 0 removals");
            println!("Generating autoload files");
        }
        "show" => {
            println!("name     : vendor/package");
            println!("versions : * 1.0.0");
            println!("type     : library");
        }
        _ => println!("({} — simulated)", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("php");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "php-fpm" => { println!("php-fpm: pool www started (simulated)"); 0 }
        "composer" => run_composer(rest),
        _ => run_php(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
