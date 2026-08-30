#![deny(clippy::all)]
//! socat — personality CLI for socat, the Swiss-army-knife
//! "multipurpose relay" by Gerhard Rieger that connects any two byte
//! streams of completely different types together.
//!
//! socat ("SOcket CAT") is what netcat would have grown into if netcat
//! had continued evolving in the late 1990s. Where nc has TCP, UDP,
//! and (sometimes) UNIX-socket support, socat has dozens of address
//! types — TCP, UDP, UNIX, SCTP, UDP-LITE, OpenSSL, PTY, EXEC, SYSTEM,
//! FILE, PIPE, STDIO, READLINE, IP-RAW, SOCKS, PROXY, TUN, ABSTRACT-
//! UNIX, RAWIP, IP-SENDTO, IP-RECVFROM, GOPEN, /dev/tty, the lot —
//! and supports building a bidirectional channel between any pair of
//! them with a richer per-address option language than any other
//! single tool in the Unix toolbox.
//!
//! It's the right answer to a remarkably broad set of small problems:
//! TLS-wrapping a plaintext service for testing, exposing a serial port
//! over TCP, replaying or recording network protocols, forwarding a
//! TCP socket through a SOCKS5 jump box, setting up a virtual null-
//! modem PTY pair, debugging a UNIX-socket daemon by interposing a
//! logger, smuggling a shell over a stdin/stdout pipe, and many more.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — socat multipurpose relay personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         socat at a glance: bidirectional byte-stream relay");
    println!("    history       Rieger 2001 -> 1.7.x (stable) -> 2.x (rewrite, draft)");
    println!("    syntax        socat ADDR1 ADDR2 - the basic invocation pattern");
    println!("    addresses     The major address types");
    println!("    options       Per-address options: fork, reuseaddr, su, pty, ...");
    println!("    examples      Common recipes: TCP fwd, TLS wrap, PTY pair, SOCKS jump");
    println!("    tls           OPENSSL-LISTEN / OPENSSL-CONNECT, cert verification");
    println!("    pty           PTY and pty-pair tricks");
    println!("    debug         -d / -d -d / -d -d -d / -x / -v / -t -T tuning");
    println!("    pitfalls      Forks, signal handling, binary-safe -u/-U, EOF semantics");
    println!("    alternatives  netcat (BSD, GNU, ncat), websocat, socat2, stunnel");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() {
    println!("socat 0.1.0 (Slate OS personality CLI)");
}

fn run_about() {
    println!("socat - multipurpose relay between two bidirectional byte streams.");
    println!("  Author:    Gerhard Rieger (Austria); first public release 2001.");
    println!("  Language:  C. Single statically-linkable binary.");
    println!("  License:   GPL-2.0-only.");
    println!("  URL:       http://www.dest-unreach.org/socat/");
    println!("  Premise:   netcat plus everything netcat ever wished it had.");
    println!("  Use cases: TLS wrapping, port-forwarding with logging, serial-over-IP,");
    println!("             SOCKS jumps, PTY pairs, UNIX-socket interposers, TUN/TAP");
    println!("             plumbing, raw IP, protocol replay, ad-hoc daemon scaffolds.");
    println!("  Footprint: ~200 KiB stripped binary, no runtime config files.");
}

fn run_history() {
    println!("Project history.");
    println!("  Jan 2001:    socat 1.0 first public release.");
    println!("  2002-2005:   1.4 / 1.5 - OpenSSL address, PTY pair, IPv6.");
    println!("  2006:        1.6.0 - reorganised codebase, SCTP support.");
    println!("  2007:        1.7.0 - new option model, lots of new address types.");
    println!("  2014-2015:   GHOST glibc and OpenSSL Logjam mitigations.");
    println!("  Ongoing:     1.7.x maintenance line; 1.7.4.4 (2023) is a common");
    println!("               stable version in distros.");
    println!("  Future:      2.x has been in beta for over a decade. Cleaner address");
    println!("               grammar, programmable filters, native loadable modules.");
    println!("               Not yet broadly adopted; 1.7.x remains the recommended");
    println!("               production line.");
}

fn run_syntax() {
    println!("Invocation syntax.");
    println!("  socat [global-options] ADDRESS1 ADDRESS2");
    println!();
    println!("  socat opens both addresses (in parallel where possible), then");
    println!("  shovels bytes from ADDRESS1->ADDRESS2 and ADDRESS2->ADDRESS1.");
    println!("  Either direction's EOF can end the program; -u / -U make the");
    println!("  relay unidirectional.");
    println!();
    println!("  Address syntax:");
    println!("      TYPE:arg1[:arg2[:...]][,option1[,option2...]]");
    println!("  e.g.");
    println!("      TCP:127.0.0.1:8080");
    println!("      TCP-LISTEN:8080,reuseaddr,fork");
    println!("      OPENSSL:host:443,verify=1,cafile=/etc/ssl/certs/ca-bundle.crt");
    println!("      UNIX-CONNECT:/var/run/foo.sock");
    println!("      EXEC:'/bin/sh',pty,setsid,stderr");
    println!("      PTY,raw,echo=0,link=/tmp/ttyV0");
    println!();
    println!("Global flags worth knowing: -d (debug; repeat for more), -v (binary");
    println!("dump traffic), -x (hex), -t <s> (delay after EOF on one side before");
    println!("closing the other), -T <s> (inactivity timeout), -u / -U (one-way),");
    println!("-b <n> (block size), -ly (syslog).");
}

fn run_addresses() {
    println!("Major address types.");
    println!("  TCP:host:port               TCP4 + TCP6 - connect.");
    println!("  TCP4 / TCP6                 v4-only / v6-only flavour.");
    println!("  TCP-LISTEN:port             TCP server. fork to accept many.");
    println!("  UDP:host:port               UDP datagram socket - connect-style.");
    println!("  UDP-LISTEN:port             UDP server.");
    println!("  UDP-RECVFROM / UDP-SENDTO   Per-packet variants.");
    println!("  SCTP / SCTP-LISTEN          SCTP equivalents.");
    println!("  UNIX-CONNECT:/path          UNIX domain socket - connect.");
    println!("  UNIX-LISTEN:/path           UNIX socket server.");
    println!("  UNIX-RECV / UNIX-SENDTO     Datagram UNIX-socket variants.");
    println!("  ABSTRACT-CONNECT:name       Linux abstract UNIX socket (\\0name).");
    println!("  OPENSSL:host:port           TLS client (uses OpenSSL).");
    println!("  OPENSSL-LISTEN:port         TLS server.");
    println!("  STDIO / STDIN / STDOUT      The process's standard streams.");
    println!("  -                           Shorthand for STDIO.");
    println!("  FILE:/path                  Plain file (read+write).");
    println!("  PIPE / PIPE:/path           anonymous pipe or named FIFO.");
    println!("  EXEC:'cmd args'             Fork+exec a child, talk on its stdio.");
    println!("  SYSTEM:'shell pipeline'     Same via /bin/sh -c.");
    println!("  PTY                         Allocate a PTY pair, return master FD.");
    println!("                              link=PATH option creates a symlink to slave.");
    println!("  READLINE                    GNU readline wrapper for interactive use.");
    println!("  GOPEN:/path                 Generic open - pick STREAMS/CHAR/PIPE/FILE.");
    println!("  IP4:host:proto              Raw IPv4 socket (proto = number).");
    println!("  IP-SENDTO / IP-RECVFROM     One-shot send / recv variants.");
    println!("  SOCKS:proxy:host:port       SOCKS4(a) client.");
    println!("  SOCKS5 / SOCKS5-CONNECT     SOCKS5 client.");
    println!("  PROXY:proxy:host:port       HTTP CONNECT proxy client.");
    println!("  TUN[:ifaddr/mask]           TUN device (IP).");
    println!("  /dev/ttyS0                  Bare device file shortcut for serial.");
}

fn run_options() {
    println!("Address options - the qualifier after the comma in TYPE:args,opt,opt.");
    println!("  Listening / forking:");
    println!("    reuseaddr           SO_REUSEADDR on the listen socket.");
    println!("    reuseport           SO_REUSEPORT.");
    println!("    fork                Fork a child per accept; parent loops.");
    println!("    max-children=N      Bound the number of concurrent children.");
    println!("    backlog=N           listen(2) backlog.");
    println!("    accept-timeout=S    Bail after S seconds of no connection.");
    println!("  Privilege:");
    println!("    su=user             setuid after open.");
    println!("    su-d=user           setuid+setgid after the open of the *second*");
    println!("                        address (often used to drop after a privileged bind).");
    println!("    chroot=/path        chroot before the second address opens.");
    println!("  PTY:");
    println!("    pty                 Run the EXEC in a PTY rather than a pipe.");
    println!("    setsid              setsid() so the child gets a fresh session.");
    println!("    ctty                Set controlling tty (lets Ctrl-C reach the child).");
    println!("    stderr              Merge child stderr into the relay (otherwise lost).");
    println!("    raw                 Disable kernel TTY line discipline.");
    println!("    echo=0              Turn off local echo.");
    println!("    link=/tmp/ttyV0     Symlink the slave path here (PTY pair recipe).");
    println!("  OpenSSL:");
    println!("    verify=0|1          Set SSL_VERIFY_PEER off/on.");
    println!("    cafile=PATH         Trust roots PEM bundle.");
    println!("    capath=DIR          Trust roots c_rehash dir.");
    println!("    cert=PATH           Local cert (PEM).");
    println!("    key=PATH            Local privkey.");
    println!("    cipher=SPEC         OpenSSL cipher string.");
    println!("    method=TLS1.2|TLS1.3 Force protocol version.");
    println!("    snihost=name        Send this SNI.");
    println!("    commonname=name     Require this CN/SAN in the peer cert.");
    println!("    fips=1              Enable FIPS provider (if built with).");
    println!("  Network:");
    println!("    bind=ip[:port]      Source address selection.");
    println!("    pf=ip4|ip6          Force AF.");
    println!("    keepalive           SO_KEEPALIVE.");
    println!("    nodelay             TCP_NODELAY.");
    println!("    so-broadcast        SO_BROADCAST for UDP.");
    println!("    ip-multicast-loop   IP_MULTICAST_LOOP.");
    println!("  Files / FDs:");
    println!("    o-creat / o-append / o-trunc   open(2) flags.");
    println!("    mode=0644           Mode for created files.");
    println!("    user=alice / group=g           Owner of created file.");
}

fn run_examples() {
    println!("Recipes.");
    println!();
    println!("  Forwarding TCP from 1234 to host:80:");
    println!("    socat TCP-LISTEN:1234,fork,reuseaddr TCP:host:80");
    println!();
    println!("  TLS-terminate to a plain backend (test server):");
    println!("    socat OPENSSL-LISTEN:8443,fork,reuseaddr,cert=server.pem,key=server.key,verify=0 \\");
    println!("          TCP:127.0.0.1:8080");
    println!();
    println!("  TLS-wrap a client (send TLS, get plain on stdio):");
    println!("    socat - OPENSSL:example.com:443,verify=1,cafile=/etc/ssl/certs/ca-bundle.crt");
    println!();
    println!("  Expose a serial port over TCP:");
    println!("    socat /dev/ttyUSB0,raw,echo=0,b115200 TCP-LISTEN:7000,fork,reuseaddr");
    println!();
    println!("  Virtual null-modem (two linked PTYs that talk to each other):");
    println!("    socat -d -d PTY,raw,echo=0,link=/tmp/ttyV0 PTY,raw,echo=0,link=/tmp/ttyV1");
    println!();
    println!("  UNIX socket -> TCP for poking a daemon from elsewhere:");
    println!("    socat UNIX-LISTEN:/tmp/relay.sock,fork,reuseaddr UNIX-CONNECT:/run/foo.sock");
    println!();
    println!("  TCP -> UDP gateway (one-way):");
    println!("    socat -u TCP-LISTEN:5514,fork UDP-SENDTO:syslog.example.com:514");
    println!();
    println!("  Shell over a single TCP port (the danger zone - listen ONLY on localhost):");
    println!("    socat TCP-LISTEN:6666,bind=127.0.0.1,reuseaddr EXEC:'/bin/sh',pty,stderr,setsid,ctty");
    println!();
    println!("  Through a SOCKS5 jump:");
    println!("    socat - SOCKS5:jump.example.net:target.internal:22,socksport=1080");
    println!();
    println!("  Mirror traffic through a logger (tee with two destinations):");
    println!("    socat -d -d -d TCP-LISTEN:8000,fork SYSTEM:'tee /tmp/in.log | nc backend 8000 | tee /tmp/out.log'");
}

fn run_tls() {
    println!("OpenSSL / TLS notes.");
    println!("  Build:   socat must be compiled with OpenSSL support (default in");
    println!("           every distro package).");
    println!("  Client:  OPENSSL:host:port plus");
    println!("             verify=1            Force certificate validation.");
    println!("             cafile=PATH         Trust anchor.");
    println!("             commonname=name     Require this CN/SAN.");
    println!("             snihost=name        Override SNI (helpful when the");
    println!("                                 server you connect to is multi-tenant).");
    println!("  Server:  OPENSSL-LISTEN:port plus");
    println!("             cert=server.pem     Server certificate (PEM).");
    println!("             key=server.key      Server private key.");
    println!("             cafile=client-ca.pem  For mTLS (client-cert verify).");
    println!("             verify=1            Require client cert (mTLS).");
    println!("             cipher=...          Restrict ciphersuites.");
    println!("             method=TLS1.3       Force TLS 1.3 only.");
    println!("  Common bug: verify=0 silently disables certificate validation -");
    println!("              fine for local tests but a footgun in production.");
    println!("  Quick PKI: openssl req -x509 -newkey rsa:2048 -days 1 -nodes \\");
    println!("                -subj '/CN=localhost' -keyout key.pem -out cert.pem");
}

fn run_pty() {
    println!("PTY tricks.");
    println!("  PTY address allocates a master/slave pair, gives socat the master,");
    println!("  and lets you control the slave from another tool.");
    println!();
    println!("  link=PATH: create a stable filesystem path for the slave so other");
    println!("             programs can open it as if it were /dev/ttySn. Useful for");
    println!("             handing a virtual serial port to QEMU, modem emulators,");
    println!("             marine-NMEA tooling, etc.");
    println!();
    println!("  Null-modem (two virtual serial ports wired together):");
    println!("    socat -d -d PTY,raw,echo=0,link=/tmp/ttyV0 PTY,raw,echo=0,link=/tmp/ttyV1");
    println!("    Now anything writing to /tmp/ttyV0 is delivered to whatever opened");
    println!("    /tmp/ttyV1, and vice-versa.");
    println!();
    println!("  Sticky options to remember:");
    println!("    raw       Disables line-discipline canon-mode/echo/^C handling.");
    println!("    echo=0    Turn off local echo (you almost always want this on the");
    println!("              slave side).");
    println!("    setsid    For PTY+EXEC: give the child a fresh session.");
    println!("    ctty      Make the PTY the child's controlling TTY so Ctrl-C signals it.");
    println!("    stderr    Without this, the child's stderr is discarded.");
    println!();
    println!("  Expose a Linux serial-port over the network exactly:");
    println!("    socat /dev/ttyS0,b115200,raw,echo=0 TCP-LISTEN:7000,fork");
    println!("  And on the client side:");
    println!("    socat - TCP:host:7000");
}

fn run_debug() {
    println!("Debugging.");
    println!("  -d        Print errors (informational). Default prints warnings only.");
    println!("  -d -d     Add notice messages.");
    println!("  -d -d -d  Add info messages.");
    println!("  -d -d -d -d  Add debug messages (very verbose - per syscall).");
    println!("  -ly       Mirror messages to syslog as well as stderr.");
    println!("  -v        Print transferred data to stderr (text).");
    println!("  -x        Print transferred data to stderr as hexdump.");
    println!("  -t <s>    After one side EOFs, wait this many seconds before closing");
    println!("            the other. Default 0.5 s. Tune up for half-duplex protocols.");
    println!("  -T <s>    Idle timeout: drop the relay after s seconds of no traffic.");
    println!("  -b <n>    Buffer / block size for each direction.");
    println!("  -u        Unidirectional ADDRESS1 -> ADDRESS2 only.");
    println!("  -U        Unidirectional ADDRESS2 -> ADDRESS1 only.");
    println!("  -V        Print exit codes from EXEC/SYSTEM children.");
    println!();
    println!("Recipe: capture full TLS-decrypted traffic between a client and a server:");
    println!("    socat -d -d -x -v OPENSSL-LISTEN:4443,reuseaddr,cert=s.pem,key=s.key \\");
    println!("                        OPENSSL:upstream.example.com:443");
}

fn run_pitfalls() {
    println!("Gotchas.");
    println!("  1. Forgotten reuseaddr.");
    println!("     A TCP-LISTEN without reuseaddr leaves the port in TIME_WAIT");
    println!("     after each child closes; the next restart fails with EADDRINUSE.");
    println!("     Always add ',reuseaddr' for repeated relays.");
    println!("  2. fork without max-children.");
    println!("     A public-facing TCP-LISTEN with ,fork has no rate limit; a");
    println!("     trivial connection flood will spawn unbounded children. Always");
    println!("     pair with ,max-children=N for production.");
    println!("  3. -t default eats short responses.");
    println!("     For HTTP/1.0 where the server response signal is EOF on close,");
    println!("     a 0.5 s -t is fine. For request-response over a single TCP");
    println!("     connection where the client sends a request and waits, -t 0");
    println!("     can chop off the response. Tune up.");
    println!("  4. EXEC child not getting a TTY.");
    println!("     Without ',pty,ctty,setsid', interactive programs (bash, vim,");
    println!("     ssh) misbehave because they detect they're not on a tty.");
    println!("  5. Binary safety: -v vs -u.");
    println!("     -v is for debugging - it prints bytes to stderr but keeps the");
    println!("     relay bidirectional. -u / -U are the one-way switches; do NOT");
    println!("     confuse them.");
    println!("  6. EOF asymmetry.");
    println!("     TCP delivers EOF on either side; UDP doesn't. A TCP->UDP relay");
    println!("     can't shut the UDP side down on client disconnect; an idle");
    println!("     timeout (-T) is how you bound that resource.");
    println!("  7. OpenSSL verify=0.");
    println!("     The default for OPENSSL-LISTEN is verify=0 (no client cert");
    println!("     required); the default for OPENSSL: client is verify=1.");
    println!("     The asymmetry confuses people. Be explicit.");
    println!("  8. UNIX-LISTEN doesn't unlink on exit.");
    println!("     The socket file is left dangling after socat exits - a restart");
    println!("     will fail until you rm it. Use ',unlink-early' or wrap in a");
    println!("     systemd unit with ExecStartPre=-/bin/rm -f /run/foo.sock.");
    println!("  9. Signal handling under fork.");
    println!("     SIGCHLD reaping is automatic for the parent listener; SIGTERM");
    println!("     on the parent kills all children. SIGHUP is not specially handled.");
}

fn run_alternatives() {
    println!("Alternatives and related tools.");
    println!("  netcat (BSD)    The original nc. TCP, UDP, UNIX, a few flags.");
    println!("                  Wonderful for quick connectivity tests; falls short");
    println!("                  the moment you need TLS, multiple types, or fork.");
    println!("  netcat (GNU)    Original GNU rewrite. Different argument grammar,");
    println!("                  different defaults. Avoid - the BSD one is the");
    println!("                  modern lingua franca.");
    println!("  ncat            Nmap's nc - TLS support, SOCKS proxying, --exec,");
    println!("                  --chat, deny/allow lists. The closest single-tool");
    println!("                  competitor to socat for TLS-flavoured tasks.");
    println!("  websocat        Rust websocket client/server in nc-flavour. The");
    println!("                  right answer when one side of the relay is ws://.");
    println!("  stunnel         TLS-only wrapper, daemon-style with config files.");
    println!("                  Heavier than socat but with mature service-manager");
    println!("                  integration, pid files, FIPS, etc. Production TLS");
    println!("                  termination jobs often pick stunnel + a backend.");
    println!("  socat2          The 2.x reboot of socat itself. Cleaner address");
    println!("                  grammar, programmable filters. Still in beta.");
    println!("  haproxy / nginx Stream-mode reverse proxies; bigger configs but");
    println!("                  better suited to TCP/UDP/TLS-termination at scale.");
    println!("  rinetd          Tiny TCP redirector daemon with a config file.");
    println!("                  Ancient but still ships in distros for the");
    println!("                  fire-and-forget case.");
    println!("  Caddy           HTTPS reverse proxy that handles ACME inline.");
    println!("                  Different problem; sometimes the right answer anyway.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "socat".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "syntax" => run_syntax(),
        "addresses" => run_addresses(),
        "options" => run_options(),
        "examples" => run_examples(),
        "tls" => run_tls(),
        "pty" => run_pty(),
        "debug" => run_debug(),
        "pitfalls" => run_pitfalls(),
        "alternatives" => run_alternatives(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs_all_subcommands() {
        run_about();
        run_history();
        run_syntax();
        run_addresses();
        run_options();
        run_examples();
        run_tls();
        run_pty();
        run_debug();
        run_pitfalls();
        run_alternatives();
    }

    #[test]
    fn help_and_version() {
        print_help("socat");
        print_version();
    }
}
