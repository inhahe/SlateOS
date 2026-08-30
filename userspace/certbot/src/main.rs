#![deny(clippy::all)]
//! certbot — personality CLI for Certbot, the EFF/ISRG-maintained ACME
//! client for obtaining and renewing TLS certificates from Let's Encrypt
//! and other ACME-compatible certificate authorities.
//!
//! Certbot started life as `letsencrypt-auto`, the official client
//! shipped alongside the Let's Encrypt public beta in September 2015.
//! It was renamed Certbot in May 2016 when the EFF took stewardship —
//! the rename also reflected the goal of supporting ACME more broadly
//! (the protocol was standardised as RFC 8555 in March 2019), not just
//! the one CA. Written in Python, it pioneered a plugin model for the
//! several pieces ACME needs from the local box: an *authenticator*
//! (proves you control the name being asked for — webroot, standalone,
//! nginx, apache, dns-cloudflare, …) and an *installer* (deploys the
//! issued cert to the right place — nginx, apache, …). The default
//! storage layout `/etc/letsencrypt/{accounts,live,archive,renewal}`
//! and the `certbot renew` cron/timer flow are the de-facto reference
//! that most other clients copy.
//!
//! Let's Encrypt itself is operated by ISRG (Internet Security Research
//! Group), a US 501(c)(3) founded in 2013 specifically to run a free,
//! automated CA. Sponsors include Mozilla, EFF, Akamai, Cisco, Chrome,
//! Facebook, IdenTrust (cross-sign), AWS, and many more. As of 2024
//! Let's Encrypt issues > 5M certificates per day and serves > 400M
//! distinct domains — the single largest CA in the WebPKI.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Certbot / Let's Encrypt ACME-client personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Certbot at a glance: ACME client, Python, EFF/ISRG");
    println!("    history       letsencrypt-auto 2015 -> Certbot 2016 -> RFC 8555 (2019)");
    println!("    acme          ACME v2 protocol: orders, authorizations, challenges");
    println!("    challenges    HTTP-01, DNS-01, TLS-ALPN-01 - when to use which");
    println!("    plugins       Authenticator + installer plugin model");
    println!("    cli           Common subcommands: certonly, run, renew, revoke...");
    println!("    layout        /etc/letsencrypt directory structure + renewal conf");
    println!("    renewal       Cron / systemd timer, --post-hook, --deploy-hook");
    println!("    wildcards     Wildcard certs since 2018 (DNS-01 only)");
    println!("    ratelimits    Let's Encrypt rate-limit table");
    println!("    isrg          Internet Security Research Group: sponsors, scale");
    println!("    boulder       Boulder - Let's Encrypt's Go ACME-server impl");
    println!("    alternatives  acme.sh, lego, dehydrated, win-acme, acme-tiny");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() {
    println!("certbot 0.1.0 (Slate OS personality CLI)");
}

fn run_about() {
    println!("Certbot - the EFF-maintained reference ACME client.");
    println!("  Origin:      letsencrypt-auto, shipped with Let's Encrypt's public");
    println!("               beta launch on Sept 14 2015. Renamed Certbot in May 2016");
    println!("               to reflect its CA-neutral ACME-client role.");
    println!("  Language:    Python. Single CLI plus a plugin entry-point system");
    println!("               (`certbot.plugins` setuptools entry-points).");
    println!("  License:     Apache-2.0.");
    println!("  Steward:     Electronic Frontier Foundation (EFF). Let's Encrypt");
    println!("               itself is operated by the Internet Security Research");
    println!("               Group (ISRG); the two organisations cooperate but the");
    println!("               codebases are separate (certbot vs boulder).");
    println!("  Role:        the *reference* implementation of RFC 8555 (ACME v2).");
    println!("               Many distros ship it as `certbot`; Debian/Ubuntu have");
    println!("               python3-certbot and python3-certbot-{{nginx,apache,...}}");
    println!("               packages, RHEL via EPEL, etc.");
}

fn run_history() {
    println!("Project history.");
    println!("  2013:         ISRG founded by Josh Aas, Eric Rescorla, J. Alex");
    println!("                Halderman, Peter Eckersley, Stephen Schultze to run");
    println!("                a free, automated, public CA.");
    println!("  2014:         ACME v1 protocol drafted (Barnes, Hoffman-Andrews, et al.).");
    println!("  Sep 14 2015:  Let's Encrypt public beta. First cross-signed");
    println!("                intermediates issued under IdenTrust DST Root CA X3.");
    println!("  Dec 3 2015:   Open public beta - anyone can get a cert.");
    println!("  Apr 12 2016:  General availability + leave-beta.");
    println!("  May 2016:     `letsencrypt-auto` renamed Certbot; EFF takes stewardship.");
    println!("  Mar 13 2018:  Wildcard certificates GA, requiring ACME v2 and DNS-01.");
    println!("  Mar 13 2019:  RFC 8555 published - ACME v2 standardised.");
    println!("  Jun 1 2021:   ACME v1 endpoint shut down. ACME v2 only thereafter.");
    println!("  Sep 30 2021:  DST Root CA X3 expires - coordinated industry-wide");
    println!("                client-trust transition to ISRG Root X1. Old Android");
    println!("                fallout mitigated by a long DST X3 cross-sign extension.");
    println!("  Feb 2024:     Multi-perspective issuance corroboration (MPIC) GA at LE.");
    println!("  2024:         ISRG Root X2 (P-384 ECDSA) put into general production for");
    println!("                ECDSA leaves. > 5M certs/day issued.");
}

fn run_acme() {
    println!("ACME v2 protocol overview (RFC 8555).");
    println!("  Account:      JWS-signed registration; account key is an ECDSA/RSA");
    println!("                keypair held client-side. The account URL identifies");
    println!("                the subscriber in subsequent requests.");
    println!("  Newnonce:     every request body is a JWS protected by a one-shot");
    println!("                Replay-Nonce header obtained from the server.");
    println!("  Order:        POST /acme/new-order with identifiers; server returns");
    println!("                one URL per identifier (authorization).");
    println!("  Authz:        the server lists challenges (http-01, dns-01, tls-alpn-01).");
    println!("                Client picks one, posts back ready-to-validate.");
    println!("  Validation:   the CA fetches the challenge from N geographically");
    println!("                distributed perspectives (MPIC, since 2020/2024).");
    println!("  Finalize:     once all authzs are valid, POST a CSR to the order's");
    println!("                finalize URL. Server returns a Location: header pointing");
    println!("                at the issued cert when the status flips to `valid`.");
    println!("  Revoke:       POST /acme/revoke-cert with the cert + reason code.");
    println!("                May be signed with account key or cert privkey.");
    println!("  Profiles:     ACME profiles (RFC 8555 plus draft extensions) let");
    println!("                clients request short-lived (6-day) vs traditional");
    println!("                90-day validity. Let's Encrypt rolled this out in 2024.");
}

fn run_challenges() {
    println!("ACME challenge types.");
    println!("  HTTP-01:      CA fetches http://<name>/.well-known/acme-challenge/<tok>");
    println!("                expecting the file's body to be tok + '.' + acct-thumb.");
    println!("                Default; needs port 80 reachable from the internet.");
    println!("                Does NOT work for wildcard certs.");
    println!("                Used by: webroot, standalone, nginx, apache plugins.");
    println!("  DNS-01:       CA queries _acme-challenge.<name> TXT for the SHA-256");
    println!("                digest of tok + '.' + acct-thumb (base64url).");
    println!("                Required for wildcards; works behind NAT or for hosts");
    println!("                that don't run a webserver. Used by: dns-cloudflare,");
    println!("                dns-route53, dns-rfc2136 (BIND nsupdate), dns-google, ...");
    println!("  TLS-ALPN-01:  CA opens a TLS connection on port 443 with the");
    println!("                `acme-tls/1` ALPN; the server presents a self-signed");
    println!("                cert containing the challenge digest in a special");
    println!("                acmeIdentifier extension (OID 1.3.6.1.5.5.7.1.31).");
    println!("                Solves the 'port 80 blocked, no DNS API' case; widely");
    println!("                used by reverse proxies (HAProxy, Traefik, Caddy).");
    println!("  TLS-SNI-01/02: removed - design was vulnerable to shared-hosting");
    println!("                takeover (Frans Rosen / Detectify, Jan 2018).");
}

fn run_plugins() {
    println!("Plugin model.");
    println!("  Authenticator: proves control of the requested identifier.");
    println!("    webroot     - writes challenge file under a directory the");
    println!("                  already-running webserver serves.");
    println!("    standalone  - Certbot binds port 80 itself; needs the port free.");
    println!("    nginx       - temporarily mutates nginx config to serve the file,");
    println!("                  then reverts.");
    println!("    apache      - same idea for Apache httpd.");
    println!("    manual      - print instructions, wait for human + hook scripts.");
    println!("    dns-<provider> - DNS-01 plugins, one per provider:");
    println!("       dns-cloudflare, dns-route53, dns-google, dns-digitalocean,");
    println!("       dns-rfc2136 (BIND nsupdate), dns-linode, dns-luadns, dns-nsone,");
    println!("       dns-ovh, dns-gehirn, dns-sakuracloud.");
    println!("  Installer:     deploys the issued cert (chain + key) into a server.");
    println!("    nginx, apache - same plugins above also install.");
    println!("    null         - write to disk only; user wires up themselves.");
    println!("  Hooks:");
    println!("    --pre-hook, --post-hook, --deploy-hook - shell hooks per renewal.");
}

fn run_cli() {
    println!("Common subcommands.");
    println!("  certonly       - obtain a cert and write it to /etc/letsencrypt;");
    println!("                   do not install. Useful for HAProxy/Postfix/Dovecot.");
    println!("  run            - obtain + install (the default if no subcommand).");
    println!("  renew          - renew every certificate that's within ~30 days of");
    println!("                   expiry. Reads /etc/letsencrypt/renewal/*.conf and");
    println!("                   re-uses the original arguments. Idempotent - safe");
    println!("                   to run twice a day from cron/timer.");
    println!("  revoke         - revoke a specific cert, optionally with a reason");
    println!("                   code (unspecified/keyCompromise/affiliationChanged/");
    println!("                   superseded/cessationOfOperation).");
    println!("  delete         - delete a cert lineage (does NOT revoke at CA).");
    println!("  certificates   - list all locally-known cert lineages and their state.");
    println!("  register       - create an ACME account without obtaining a cert.");
    println!("  update_account - change account contact email or rotate account key.");
    println!("  unregister     - deactivate the ACME account.");
    println!("  show_account   - print account URL, contacts, registration date.");
    println!("  install        - install an existing cert into a server (with installer).");
    println!("  enhance        - enable HSTS, OCSP stapling, redirect in nginx/apache.");
    println!();
    println!("Examples:");
    println!("    certbot certonly --webroot -w /var/www -d example.com -d www.example.com");
    println!("    certbot --nginx -d example.com");
    println!("    certbot certonly --dns-cloudflare --dns-cloudflare-credentials cf.ini \\");
    println!("        -d example.com -d '*.example.com'");
    println!("    certbot renew --quiet --post-hook 'systemctl reload nginx'");
}

fn run_layout() {
    println!("Filesystem layout - /etc/letsencrypt/.");
    println!("  accounts/<server>/directory/<thumb>/  - per-CA-endpoint account info");
    println!("    private_key.json    Account JWS key (JSON).");
    println!("    regr.json           Account URL + contact list as returned by the CA.");
    println!("    meta.json           Last refresh timestamp.");
    println!("  csr/                  Historical CSRs (timestamped).");
    println!("  keys/                 Historical private keys (timestamped).");
    println!("  archive/<lineage>/    Every issued cert is appended here:");
    println!("    cert1.pem, chain1.pem, fullchain1.pem, privkey1.pem,");
    println!("    cert2.pem, chain2.pem, fullchain2.pem, privkey2.pem, ...");
    println!("  live/<lineage>/       Symlinks to the most recent archive/ files.");
    println!("    cert.pem -> ../../archive/<lineage>/certN.pem");
    println!("    Stable filenames - point your webserver here.");
    println!("  renewal/<lineage>.conf");
    println!("    INI file recording arguments used at issuance so `renew` can");
    println!("    reproduce them. Edit to change webroot, plugin, or hook config.");
    println!("  renewal-hooks/{{pre,post,deploy}}/");
    println!("    Drop scripts here; each is run on every renewal automatically.");
}

fn run_renewal() {
    println!("Renewal.");
    println!("  Schedule:   certbot is designed to be run twice a day from a system");
    println!("              timer; the `renew` subcommand only acts on certs within");
    println!("              ~30 days of expiry. Twice-a-day cadence gives 60 chances");
    println!("              to recover from a transient CA outage before the cert");
    println!("              actually expires.");
    println!("  systemd:    most distros ship certbot.timer (twice daily, randomized");
    println!("              delay) + certbot.service (Type=oneshot ExecStart=/usr/bin/certbot -q renew).");
    println!("  cron:       /etc/cron.d/certbot - `0 */12 * * * root certbot -q renew`.");
    println!("  --post-hook:    runs once per `certbot renew` invocation, if any cert");
    println!("                  was renewed. Use to reload webserver.");
    println!("  --deploy-hook:  runs once per *successfully renewed lineage*; lineage");
    println!("                  name is in $RENEWED_LINEAGE. Use for multi-service");
    println!("                  fan-out (e.g. push to Postfix, Dovecot, Prosody).");
    println!("  --pre-hook:     runs once per renew invocation BEFORE any cert work.");
    println!("                  Use to open firewall ports for standalone plugin.");
    println!("  ACME ARI:       Auto-Renewal-Information (RFC draft) - CA tells the");
    println!("                  client a *suggested* renewal window; Certbot honors it.");
    println!("                  Lets the CA stagger renewals or force-revoke fleetwide.");
}

fn run_wildcards() {
    println!("Wildcard certificates.");
    println!("  Support:    GA Mar 13 2018, simultaneously with the ACME v2 endpoint");
    println!("              transition. *.example.com matches one DNS label only -");
    println!("              foo.example.com but NOT example.com or foo.bar.example.com.");
    println!("              To cover both apex + wildcard you must list both:");
    println!("                  -d example.com -d '*.example.com'");
    println!("  Restriction:DNS-01 is the only allowed challenge for wildcards.");
    println!("              HTTP-01/TLS-ALPN-01 cannot prove control of *.x.com.");
    println!("  Tooling:    A DNS-API plugin (dns-cloudflare etc.) plus a credentials");
    println!("              file with API-token scoped to the relevant zone. Token");
    println!("              file must be chmod 600.");
    println!("  CAA:        Set CAA records to restrict who can issue. For Let's");
    println!("              Encrypt: 'example.com. CAA 0 issue \"letsencrypt.org\"'.");
    println!("              For wildcards add the issuewild attribute as well.");
}

fn run_ratelimits() {
    println!("Let's Encrypt rate limits (production, as of 2024-2025).");
    println!("  Certificates per registered domain: 50 per rolling week.");
    println!("    The registered-domain is the eTLD+1 from the Public Suffix List;");
    println!("    example.com counts the same as foo.example.com or bar.example.com.");
    println!("  Duplicate certs: 5 per identifier-set per rolling week.");
    println!("    Bumping a single name into an existing cert is not a duplicate.");
    println!("  Pending authorizations: 300 per account.");
    println!("    Lower this with completed orders; high pending is usually a bug.");
    println!("  New orders: 300 per account per 3 hours.");
    println!("  Accounts per IP: 10 per 3 hours.");
    println!("  Accounts per IPv6 /48 range: 500 per 3 hours.");
    println!("  Failed validations: 5 per account per hostname per hour.");
    println!("  Overrides: high-volume hosters can request lifts via");
    println!("             https://isrg.formstack.com/forms/rate_limit_adjustment_request.");
    println!("  Staging:   https://acme-staging-v02.api.letsencrypt.org/directory");
    println!("             - no rate limits, certs are NOT trusted; test here first.");
}

fn run_isrg() {
    println!("Internet Security Research Group (ISRG).");
    println!("  Type:       US 501(c)(3) public charity, incorporated 2013.");
    println!("  Operates:   Let's Encrypt, Divvi Up (privacy-preserving telemetry),");
    println!("              Prossimo (memory-safety in critical infra), and the");
    println!("              `boulder` ACME-server software (Go).");
    println!("  Funding:    Mozilla, EFF, Akamai, Cisco, Chrome, Facebook, Amazon,");
    println!("              Sidekick LLC, IdenTrust (cross-sign partner), Gemini,");
    println!("              Fastly, GoDaddy, AWS, OVHcloud, Trail of Bits, others.");
    println!("              Annual budget on the order of US$ 8-12M.");
    println!("  Trust:      ISRG Root X1 (RSA, 2015) and ISRG Root X2 (ECDSA P-384,");
    println!("              2020). X1 is trusted in all modern OS/browser root stores.");
    println!("              Cross-signed historically by IdenTrust DST Root CA X3");
    println!("              (expired Sep 30 2021).");
    println!("  Scale:      As of 2024, > 5M certs/day, > 400M distinct domains.");
    println!("              Single largest CA in the WebPKI by issuance volume.");
}

fn run_boulder() {
    println!("Boulder - Let's Encrypt's ACME-server implementation.");
    println!("  Language:   Go.");
    println!("  License:    MPL-2.0.");
    println!("  Repository: github.com/letsencrypt/boulder.");
    println!("  Architecture:");
    println!("    Web Front End (WFE2)  - handles ACME HTTP API surface.");
    println!("    Registration Authority (RA) - applies policy.");
    println!("    Validation Authority (VA)   - performs HTTP-01/DNS-01/TLS-ALPN-01");
    println!("                                  fetches from multiple network");
    println!("                                  perspectives (MPIC).");
    println!("    Storage Authority (SA)      - MariaDB-backed persistence.");
    println!("    CA Service                  - signs CSRs via the offline issuer.");
    println!("    Publisher                   - submits precerts to CT logs.");
    println!("    OCSP / CRL Updater          - generates revocation status data.");
    println!("    Health checker              - gating the precert/CT flow.");
    println!("  Roots/intermediates kept offline in HSMs (Thales Luna); intermediates");
    println!("  rotated regularly. Public infrastructure transparency reports are");
    println!("  published quarterly.");
}

fn run_alternatives() {
    println!("Other ACME clients.");
    println!("  acme.sh        - POSIX-shell ACME client (Neilpang). Single script,");
    println!("                   no Python runtime needed. Massive DNS-API coverage.");
    println!("                   Default on Alpine, OpenWrt, embedded boxes.");
    println!("  lego           - Go implementation by Go-acme. Used as the embedded");
    println!("                   client in Traefik, Caddy (alternative), and many SaaS.");
    println!("  dehydrated     - Bash + curl client (Lukas Schauer); ~600-line script.");
    println!("                   Popular for minimalist setups. /etc/dehydrated config.");
    println!("  win-acme       - Windows ACME client (PKISharp). IIS integration.");
    println!("  acme-tiny      - Diafygi's auditable ~200-line Python client.");
    println!("                   Designed to be reviewable line-by-line, signs CSRs");
    println!("                   without ever touching account or cert privkey.");
    println!("  Caddy          - webserver with an embedded ACME client (using lego");
    println!("                   originally, now its own certmagic library).");
    println!("                   `automatic HTTPS by default` for any configured host.");
    println!("  Traefik        - reverse proxy with embedded ACME via lego.");
    println!("  step-ca        - Smallstep's ACME server + step CLI client; lets you");
    println!("                   stand up an internal ACME CA in minutes.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "certbot".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "acme" => run_acme(),
        "challenges" => run_challenges(),
        "plugins" => run_plugins(),
        "cli" => run_cli(),
        "layout" => run_layout(),
        "renewal" => run_renewal(),
        "wildcards" => run_wildcards(),
        "ratelimits" => run_ratelimits(),
        "isrg" => run_isrg(),
        "boulder" => run_boulder(),
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
        run_acme();
        run_challenges();
        run_plugins();
        run_cli();
        run_layout();
        run_renewal();
        run_wildcards();
        run_ratelimits();
        run_isrg();
        run_boulder();
        run_alternatives();
    }

    #[test]
    fn help_and_version() {
        print_help("certbot");
        print_version();
    }
}
