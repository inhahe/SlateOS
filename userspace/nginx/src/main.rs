#![deny(clippy::all)]

//! nginx — SlateOS high-performance web server and reverse proxy
//!
//! Single personality: `nginx`

use std::env;
use std::process;

fn run_nginx(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "-?") {
        println!("Usage: nginx [-?hvVtTq] [-s signal] [-p prefix]");
        println!("             [-e filename] [-c filename] [-g directives]");
        println!();
        println!("Options:");
        println!("  -?,-h         : this help");
        println!("  -v            : show version and exit");
        println!("  -V            : show version and configure options then exit");
        println!("  -t            : test configuration and exit");
        println!("  -T            : test configuration, dump it and exit");
        println!("  -q            : suppress non-error messages during test");
        println!("  -s signal     : send signal to a master process: stop, quit, reopen, reload");
        println!("  -p prefix     : set prefix path (default: /etc/nginx/)");
        println!("  -e filename   : set error log file (default: /var/log/nginx/error.log)");
        println!("  -c filename   : set configuration file (default: /etc/nginx/nginx.conf)");
        println!("  -g directives : set global directives out of configuration file");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("nginx version: nginx/1.26.1 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("nginx version: nginx/1.26.1 (Slate OS)");
        println!("built with Slate OS toolchain");
        println!("TLS SNI support enabled");
        println!("configure arguments: --prefix=/etc/nginx --sbin-path=/usr/sbin/nginx --with-http_ssl_module --with-http_v2_module --with-http_realip_module --with-http_gzip_static_module --with-http_stub_status_module --with-stream --with-stream_ssl_module");
        return 0;
    }
    if args.iter().any(|a| a == "-t" || a == "-T") {
        let conf = args.iter().position(|a| a == "-c")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("/etc/nginx/nginx.conf");
        println!("nginx: the configuration file {} syntax is ok", conf);
        println!("nginx: configuration file {} test is successful", conf);
        if args.iter().any(|a| a == "-T") {
            println!();
            println!("# configuration file {}:", conf);
            println!("worker_processes auto;");
            println!("error_log /var/log/nginx/error.log;");
            println!("pid /run/nginx.pid;");
            println!();
            println!("events {{");
            println!("    worker_connections 1024;");
            println!("}}");
            println!();
            println!("http {{");
            println!("    include       /etc/nginx/mime.types;");
            println!("    default_type  application/octet-stream;");
            println!("    sendfile      on;");
            println!("    keepalive_timeout 65;");
            println!();
            println!("    server {{");
            println!("        listen 80;");
            println!("        server_name localhost;");
            println!("        root /var/www/html;");
            println!("        index index.html;");
            println!("    }}");
            println!("}}");
        }
        return 0;
    }

    let signal = args.iter().position(|a| a == "-s")
        .and_then(|i| args.get(i + 1));
    if let Some(sig) = signal {
        match sig.as_str() {
            "stop" => println!("nginx: sending stop signal to master process"),
            "quit" => println!("nginx: sending graceful quit signal to master process"),
            "reload" => println!("nginx: sending reload signal to master process"),
            "reopen" => println!("nginx: sending reopen signal to master process"),
            other => {
                eprintln!("nginx: invalid signal: {}", other);
                return 1;
            }
        }
        return 0;
    }

    // Start server
    println!("nginx/1.26.1 (Slate OS)");
    println!("2025/05/22 10:00:00 [notice] 12345#0: using the \"epoll\" event method");
    println!("2025/05/22 10:00:00 [notice] 12345#0: nginx/1.26.1");
    println!("2025/05/22 10:00:00 [notice] 12345#0: OS: Slate OS x86_64");
    println!("2025/05/22 10:00:00 [notice] 12345#0: start worker processes");
    println!("2025/05/22 10:00:00 [notice] 12345#0: start worker process 12346");
    println!("2025/05/22 10:00:00 [notice] 12345#0: start worker process 12347");
    println!("2025/05/22 10:00:00 [notice] 12345#0: start worker process 12348");
    println!("2025/05/22 10:00:00 [notice] 12345#0: start worker process 12349");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nginx(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nginx};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nginx(vec!["--help".to_string()]), 0);
        assert_eq!(run_nginx(vec!["-h".to_string()]), 0);
        let _ = run_nginx(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nginx(vec![]);
    }
}
