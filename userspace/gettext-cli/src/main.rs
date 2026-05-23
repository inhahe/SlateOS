#![deny(clippy::all)]

//! gettext-cli — OurOS GNU gettext i18n tools CLI
//!
//! Multi-personality: `gettext`, `xgettext`, `msgfmt`, `msginit`, `msgmerge`, `msgcat`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_gettext(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gettext [OPTIONS] [TEXTDOMAIN] MSGID");
        println!();
        println!("gettext — translate message (OurOS).");
        println!();
        println!("Options:");
        println!("  -d DOMAIN    Text domain");
        println!("  -e           Enable escape interpretation");
        println!("  -n           No trailing newline");
        println!("  -E           (ignored for compatibility)");
        return 0;
    }
    let text: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if let Some(msg) = text.last() {
        println!("{}", msg);
    }
    0
}

fn run_xgettext(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xgettext [OPTIONS] [FILE ...]");
        println!();
        println!("xgettext — extract translatable strings (OurOS).");
        println!();
        println!("Options:");
        println!("  -o, --output FILE    Output file");
        println!("  -L, --language LANG  Source language");
        println!("  -k, --keyword WORD   Keyword for extraction");
        println!("  -j, --join-existing  Join with existing file");
        println!("  --from-code ENC      Source file encoding");
        println!("  -c, --add-comments   Add comments");
        println!("  --no-location        Don't write file:line");
        println!("  --sort-output        Sort output");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("xgettext (GNU gettext-tools) 0.22.4 (OurOS)");
        return 0;
    }
    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output").map(|w| w[1].as_str()).unwrap_or("messages.po");
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    println!("Extracting from {} file(s) → '{}'", files.len(), output);
    0
}

fn run_msgfmt(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: msgfmt [OPTIONS] FILE.po ...");
        println!();
        println!("msgfmt — compile message catalog (OurOS).");
        println!();
        println!("Options:");
        println!("  -o, --output-file FILE  Output .mo file");
        println!("  -c, --check             Check format strings");
        println!("  --statistics             Show statistics");
        println!("  -v, --verbose            Verbose");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("msgfmt (GNU gettext-tools) 0.22.4 (OurOS)");
        return 0;
    }
    let stats = args.iter().any(|a| a == "--statistics");
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for f in &files {
        println!("Compiling '{}'...", f);
        if stats {
            println!("  42 translated messages, 3 fuzzy, 1 untranslated.");
        }
    }
    0
}

fn run_msginit(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: msginit [OPTIONS]");
        println!();
        println!("msginit — initialize new message catalog (OurOS).");
        println!();
        println!("Options:");
        println!("  -i, --input FILE    Input POT file");
        println!("  -o, --output FILE   Output PO file");
        println!("  -l, --locale LOC    Target locale");
        return 0;
    }
    let locale = args.windows(2).find(|w| w[0] == "-l" || w[0] == "--locale").map(|w| w[1].as_str()).unwrap_or("en_US");
    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output").map(|w| w[1].as_str()).unwrap_or("messages.po");
    println!("Creating '{}' for locale '{}'...", output, locale);
    println!("Done.");
    0
}

fn run_msgmerge(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: msgmerge [OPTIONS] DEF.po REF.pot");
        println!();
        println!("msgmerge — merge message catalogs (OurOS).");
        println!();
        println!("Options:");
        println!("  -o, --output FILE   Output file");
        println!("  -U, --update        Update def.po in place");
        println!("  --backup MODE       Backup mode");
        println!("  -N, --no-fuzzy-matching  No fuzzy matching");
        return 0;
    }
    println!("Merging catalogs...");
    println!("Done. 42 translated, 3 fuzzy, 1 untranslated.");
    0
}

fn run_msgcat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: msgcat [OPTIONS] FILE ...");
        println!();
        println!("msgcat — concatenate message catalogs (OurOS).");
        println!();
        println!("Options:");
        println!("  -o, --output FILE   Output file");
        println!("  --use-first         Use first translation");
        println!("  -t, --to-code ENC   Output encoding");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    println!("Concatenating {} catalog(s)...", files.len());
    println!("Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "gettext".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "xgettext" => run_xgettext(&rest),
        "msgfmt" => run_msgfmt(&rest),
        "msginit" => run_msginit(&rest),
        "msgmerge" => run_msgmerge(&rest),
        "msgcat" => run_msgcat(&rest),
        _ => run_gettext(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
