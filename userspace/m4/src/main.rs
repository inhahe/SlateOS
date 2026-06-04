//! OurOS `m4` -- macro processor
//!
//! A POSIX-compatible m4(1) implementation.  Reads input, identifies defined
//! macro names, collects parenthesized arguments, substitutes `$1`..`$9`,
//! `$0`, `$#`, `$*`, `$@`, and re-scans the result.  Supports diversions,
//! quote and comment character changes, arithmetic evaluation, string
//! operations, file inclusion, and the standard set of built-in macros.
//!
//! Architecture: single-pass token scanner -> macro expansion engine with
//! re-scanning -> diversion buffering -> final output assembly.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum expansion depth before we abort to prevent infinite recursion.
const MAX_EXPANSION_DEPTH: usize = 1024;

/// Maximum number of diversion buffers.
const MAX_DIVERSIONS: usize = 256;

// ---------------------------------------------------------------------------
// Command-line options
// ---------------------------------------------------------------------------

/// Parsed command-line options.
#[derive(Debug, Clone)]
#[derive(Default)]
struct Options {
    /// `-D name=value` pre-definitions.
    defines: Vec<(String, String)>,
    /// `-U name` pre-undefinitions.
    undefines: Vec<String>,
    /// `-I dir` include search paths.
    include_dirs: Vec<PathBuf>,
    /// `-s` emit `#line` sync directives.
    sync_lines: bool,
    /// `-P` prefix all builtins with `m4_`.
    prefix_builtins: bool,
    /// `-Q` disable all warnings.
    quiet: bool,
    /// Positional input files (empty = stdin).
    input_files: Vec<String>,
}


fn parse_args(args: &[String]) -> Options {
    let mut opts = Options::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-D" || arg == "--define" {
            i += 1;
            if i < args.len() {
                let spec = &args[i];
                if let Some(eq) = spec.find('=') {
                    opts.defines
                        .push((spec[..eq].to_string(), spec[eq + 1..].to_string()));
                } else {
                    opts.defines.push((spec.clone(), String::new()));
                }
            }
        } else if let Some(rest) = arg.strip_prefix("-D") {
            if let Some(eq) = rest.find('=') {
                opts.defines
                    .push((rest[..eq].to_string(), rest[eq + 1..].to_string()));
            } else {
                opts.defines.push((rest.to_string(), String::new()));
            }
        } else if arg == "-U" || arg == "--undefine" {
            i += 1;
            if i < args.len() {
                opts.undefines.push(args[i].clone());
            }
        } else if let Some(rest) = arg.strip_prefix("-U") {
            opts.undefines.push(rest.to_string());
        } else if arg == "-I" {
            i += 1;
            if i < args.len() {
                opts.include_dirs.push(PathBuf::from(&args[i]));
            }
        } else if let Some(rest) = arg.strip_prefix("-I") {
            opts.include_dirs.push(PathBuf::from(rest));
        } else if arg == "-s" {
            opts.sync_lines = true;
        } else if arg == "-P" {
            opts.prefix_builtins = true;
        } else if arg == "-Q" {
            opts.quiet = true;
        } else if arg == "--" {
            // Everything after `--` is an input file.
            for f in &args[i + 1..] {
                opts.input_files.push(f.clone());
            }
            break;
        } else if arg.starts_with('-') && arg.len() > 1 {
            eprintln!("m4: unknown option: {arg}");
        } else {
            opts.input_files.push(arg.clone());
        }
        i += 1;
    }
    opts
}

// ---------------------------------------------------------------------------
// Built-in identifiers
// ---------------------------------------------------------------------------

/// All built-in macro names.  Order matters for indexing — keep in sync with
/// the `Builtin` enum discriminant values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Builtin {
    Define,
    Undefine,
    Defn,
    Ifdef,
    Ifelse,
    Shift,
    Changequote,
    Changecom,
    Dnl,
    Divert,
    Undivert,
    Divnum,
    Len,
    Index,
    Substr,
    Translit,
    Incr,
    Decr,
    Eval,
    Syscmd,
    Sysval,
    Maketemp,
    Errprint,
    Dumpdef,
    Include,
    Sinclude,
    Format,
    Regexp,
    Patsubst,
}

/// Return the list of `(name, builtin)` pairs.  When `-P` is active the
/// caller prepends `m4_` to each name.
fn builtin_names() -> Vec<(&'static str, Builtin)> {
    vec![
        ("define", Builtin::Define),
        ("undefine", Builtin::Undefine),
        ("defn", Builtin::Defn),
        ("ifdef", Builtin::Ifdef),
        ("ifelse", Builtin::Ifelse),
        ("shift", Builtin::Shift),
        ("changequote", Builtin::Changequote),
        ("changecom", Builtin::Changecom),
        ("dnl", Builtin::Dnl),
        ("divert", Builtin::Divert),
        ("undivert", Builtin::Undivert),
        ("divnum", Builtin::Divnum),
        ("len", Builtin::Len),
        ("index", Builtin::Index),
        ("substr", Builtin::Substr),
        ("translit", Builtin::Translit),
        ("incr", Builtin::Incr),
        ("decr", Builtin::Decr),
        ("eval", Builtin::Eval),
        ("syscmd", Builtin::Syscmd),
        ("sysval", Builtin::Sysval),
        ("maketemp", Builtin::Maketemp),
        ("errprint", Builtin::Errprint),
        ("dumpdef", Builtin::Dumpdef),
        ("include", Builtin::Include),
        ("sinclude", Builtin::Sinclude),
        ("format", Builtin::Format),
        ("regexp", Builtin::Regexp),
        ("patsubst", Builtin::Patsubst),
    ]
}

// ---------------------------------------------------------------------------
// Macro table entry
// ---------------------------------------------------------------------------

/// A macro definition — either a user-defined text body or a built-in.
#[derive(Debug, Clone)]
enum MacroDef {
    /// User-defined macro with a text body containing `$` references.
    User(String),
    /// Built-in macro.
    BuiltinDef(Builtin),
}

// ---------------------------------------------------------------------------
// Processor state
// ---------------------------------------------------------------------------

/// The m4 processing engine.
struct Processor {
    /// Macro table: name -> stack of definitions (most recent last).
    macros: HashMap<String, Vec<MacroDef>>,
    /// Current quote open/close strings.
    quote_open: String,
    quote_close: String,
    /// Current comment open/close strings.
    comment_open: String,
    comment_close: String,
    /// Diversion buffers.  Index 0 is the "normal" output.
    diversions: Vec<String>,
    /// Current diversion number.
    current_diversion: i32,
    /// Include search path.
    include_dirs: Vec<PathBuf>,
    /// Exit status of last `syscmd`.
    last_sysval: i32,
    /// Whether to emit `#line` sync directives.
    sync_lines: bool,
    /// Disable warnings.
    quiet: bool,
    /// Current expansion depth (for recursion guard).
    depth: usize,
    /// Stderr output collector (for testing).
    #[cfg(test)]
    stderr_buf: String,
    /// Counter for `maketemp`.
    temp_counter: u64,
}

impl Processor {
    fn new() -> Self {
        let mut p = Self {
            macros: HashMap::new(),
            quote_open: "`".to_string(),
            quote_close: "'".to_string(),
            comment_open: "#".to_string(),
            comment_close: "\n".to_string(),
            diversions: vec![String::new()],
            current_diversion: 0,
            include_dirs: Vec::new(),
            last_sysval: 0,
            sync_lines: false,
            quiet: false,
            depth: 0,
            #[cfg(test)]
            stderr_buf: String::new(),
            temp_counter: 0,
        };
        // Register all builtins.
        for (name, bi) in builtin_names() {
            p.macros
                .insert(name.to_string(), vec![MacroDef::BuiltinDef(bi)]);
        }
        p
    }

    /// Create a processor with `-P` (prefix builtins) mode.
    fn new_with_prefix() -> Self {
        let mut p = Self {
            macros: HashMap::new(),
            quote_open: "`".to_string(),
            quote_close: "'".to_string(),
            comment_open: "#".to_string(),
            comment_close: "\n".to_string(),
            diversions: vec![String::new()],
            current_diversion: 0,
            include_dirs: Vec::new(),
            last_sysval: 0,
            sync_lines: false,
            quiet: false,
            depth: 0,
            #[cfg(test)]
            stderr_buf: String::new(),
            temp_counter: 0,
        };
        for (name, bi) in builtin_names() {
            let prefixed = format!("m4_{name}");
            p.macros.insert(prefixed, vec![MacroDef::BuiltinDef(bi)]);
        }
        p
    }

    // -----------------------------------------------------------------------
    // Output helpers
    // -----------------------------------------------------------------------

    /// Write text to the current diversion buffer.
    fn output(&mut self, text: &str) {
        if self.current_diversion < 0 {
            // Diversion -1 (or any negative) discards output.
            return;
        }
        let idx = self.current_diversion as usize;
        while self.diversions.len() <= idx {
            self.diversions.push(String::new());
        }
        self.diversions[idx].push_str(text);
    }

    /// Write a warning to stderr (or test buffer).
    fn warn(&mut self, msg: &str) {
        if self.quiet {
            return;
        }
        #[cfg(test)]
        {
            self.stderr_buf.push_str("m4: ");
            self.stderr_buf.push_str(msg);
            self.stderr_buf.push('\n');
        }
        #[cfg(not(test))]
        {
            let _ = writeln!(io::stderr(), "m4: {msg}");
        }
    }

    /// Write to stderr unconditionally (for `errprint` / `dumpdef`).
    fn errprint(&mut self, msg: &str) {
        #[cfg(test)]
        {
            self.stderr_buf.push_str(msg);
        }
        #[cfg(not(test))]
        {
            let _ = write!(io::stderr(), "{msg}");
        }
    }

    // -----------------------------------------------------------------------
    // Main expansion entry point
    // -----------------------------------------------------------------------

    /// Process `input` and return the collected output (diversion 0 with
    /// all pending diversions appended).
    fn process(&mut self, input: &str) -> String {
        self.expand_string(input);
        self.flush_diversions()
    }

    /// Assemble final output: diversion 0, then all positive diversions in
    /// order.  Diversions 1+ are appended after diversion 0.
    fn flush_diversions(&mut self) -> String {
        let mut result = String::new();
        if !self.diversions.is_empty() {
            result.push_str(&self.diversions[0]);
        }
        for i in 1..self.diversions.len() {
            result.push_str(&self.diversions[i]);
        }
        result
    }

    // -----------------------------------------------------------------------
    // Scanner / expander
    // -----------------------------------------------------------------------

    /// Expand the given string, writing results to the current diversion.
    /// Handles `dnl` correctly by discarding input up to and including the
    /// next newline after `dnl` invocation.
    fn expand_string(&mut self, input: &str) {
        if self.depth > MAX_EXPANSION_DEPTH {
            self.warn("recursion limit exceeded");
            return;
        }
        self.depth += 1;

        let chars: Vec<char> = input.chars().collect();
        let len = chars.len();
        let mut pos = 0;

        while pos < len {
            // 1. Try to match comment opening.
            if !self.comment_open.is_empty() && self.starts_with_at(&chars, pos, &self.comment_open)
            {
                let com_close = self.comment_close.clone();
                let com_open = self.comment_open.clone();
                self.output(&com_open);
                pos += com_open.len();
                loop {
                    if pos >= len {
                        break;
                    }
                    if self.starts_with_at(&chars, pos, &com_close) {
                        self.output(&com_close);
                        pos += com_close.len();
                        break;
                    }
                    self.output(&chars[pos].to_string());
                    pos += 1;
                }
                continue;
            }

            // 2. Try to match quote opening.
            if !self.quote_open.is_empty() && self.starts_with_at(&chars, pos, &self.quote_open) {
                let text = self.scan_quoted(&chars, &mut pos);
                self.output(&text);
                continue;
            }

            // 3. Try to match an identifier (potential macro name).
            if is_id_start(chars[pos]) {
                let start = pos;
                while pos < len && is_id_continue(chars[pos]) {
                    pos += 1;
                }
                let name: String = chars[start..pos].iter().collect();

                if let Some(def) = self.lookup_macro(&name) {
                    // Check if this is dnl.
                    let is_dnl = matches!(&def, MacroDef::BuiltinDef(Builtin::Dnl));

                    let raw_args = self.collect_args(&chars, &mut pos);
                    // POSIX m4: arguments are expanded before being passed.
                    let args = self.expand_args(&raw_args);
                    self.invoke_macro(&name, &def, &args);

                    if is_dnl {
                        // Discard everything up to and including the next
                        // newline.
                        while pos < len && chars[pos] != '\n' {
                            pos += 1;
                        }
                        if pos < len && chars[pos] == '\n' {
                            pos += 1;
                        }
                    }
                } else {
                    self.output(&name);
                }
                continue;
            }

            // 4. Ordinary character — pass through.
            self.output(&chars[pos].to_string());
            pos += 1;
        }

        self.depth -= 1;
    }

    /// Check if the char slice at `pos` starts with `needle`.
    fn starts_with_at(&self, chars: &[char], pos: usize, needle: &str) -> bool {
        let needle_chars: Vec<char> = needle.chars().collect();
        if pos + needle_chars.len() > chars.len() {
            return false;
        }
        for (i, &nc) in needle_chars.iter().enumerate() {
            if chars[pos + i] != nc {
                return false;
            }
        }
        true
    }

    /// Scan a quoted string starting at `pos` (which points at the quote-open
    /// delimiter).  Returns the text *inside* the outermost quotes (with
    /// nested quotes preserved).  `pos` is advanced past the closing quote.
    fn scan_quoted(&self, chars: &[char], pos: &mut usize) -> String {
        let qo = self.quote_open.clone();
        let qc = self.quote_close.clone();
        let qo_len = qo.chars().count();
        let qc_len = qc.chars().count();

        // Skip opening quote.
        *pos += qo_len;

        let mut depth = 1usize;
        let mut result = String::new();
        let len = chars.len();

        while *pos < len && depth > 0 {
            if self.starts_with_at(chars, *pos, &qo) {
                depth += 1;
                result.push_str(&qo);
                *pos += qo_len;
            } else if self.starts_with_at(chars, *pos, &qc) {
                depth -= 1;
                if depth > 0 {
                    result.push_str(&qc);
                }
                *pos += qc_len;
            } else {
                result.push(chars[*pos]);
                *pos += 1;
            }
        }
        result
    }

    /// Collect macro arguments.  If the next non-whitespace character is `(`,
    /// collect comma-separated arguments respecting nested parentheses and
    /// quotes.  Otherwise return an empty argument list.
    fn collect_args(&self, chars: &[char], pos: &mut usize) -> Vec<String> {
        let len = chars.len();
        // Peek ahead for '(' — but do NOT skip whitespace before it.
        // POSIX m4: arguments collected only if '(' immediately follows
        // the macro name (possibly with whitespace in between — GNU m4
        // does skip whitespace).  We follow GNU behaviour here.
        let saved = *pos;
        let mut peek = *pos;
        while peek < len && (chars[peek] == ' ' || chars[peek] == '\t') {
            peek += 1;
        }
        if peek >= len || chars[peek] != '(' {
            *pos = saved;
            return Vec::new();
        }
        *pos = peek + 1; // skip '('

        let mut args: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut paren_depth = 1u32;

        // Skip leading whitespace of the first argument (GNU m4 behaviour).
        while *pos < len && (chars[*pos] == ' ' || chars[*pos] == '\t') {
            *pos += 1;
        }

        while *pos < len {
            // Check for quote.  Preserve one level of quoting in the collected
            // argument text: argument collection and macro expansion are a
            // single logical pass in m4, but this implementation collects raw
            // args first and expands them via `expand_args`.  If we stripped
            // the quotes here, that later expansion pass would re-scan the
            // (now unquoted) text and wrongly expand it — e.g. the `foo` in
            // `define(`x', defn(`foo'))` or the name in `undefine(`x')` would
            // be expanded as a macro instead of taken literally.  Re-wrapping
            // with the quote delimiters lets `expand_args` strip exactly one
            // level and treat the contents literally.
            if !self.quote_open.is_empty() && self.starts_with_at(chars, *pos, &self.quote_open) {
                let quoted = self.scan_quoted(chars, pos);
                current.push_str(&self.quote_open);
                current.push_str(&quoted);
                current.push_str(&self.quote_close);
                continue;
            }

            let ch = chars[*pos];

            if ch == '(' {
                paren_depth += 1;
                current.push('(');
                *pos += 1;
            } else if ch == ')' {
                paren_depth -= 1;
                if paren_depth == 0 {
                    *pos += 1;
                    args.push(current);
                    break;
                }
                current.push(')');
                *pos += 1;
            } else if ch == ',' && paren_depth == 1 {
                args.push(current);
                current = String::new();
                *pos += 1;
                // Skip leading whitespace of the next argument.
                while *pos < len && (chars[*pos] == ' ' || chars[*pos] == '\t') {
                    *pos += 1;
                }
            } else {
                current.push(ch);
                *pos += 1;
            }
        }

        args
    }

    /// Look up a macro by name.  Returns a clone of the most recent
    /// definition if present.
    fn lookup_macro(&self, name: &str) -> Option<MacroDef> {
        self.macros
            .get(name)
            .and_then(|stack| stack.last().cloned())
    }

    /// Expand a string and capture the output into a new `String`, rather
    /// than writing to the current diversion.  Used to expand macro
    /// arguments before passing them to the macro.
    fn expand_to_string(&mut self, input: &str) -> String {
        // Save diversion state and redirect to a temporary buffer.
        let saved_diversion = self.current_diversion;
        let temp_idx = self.diversions.len();
        self.diversions.push(String::new());
        self.current_diversion = temp_idx as i32;

        self.expand_string(input);

        // Collect the result and restore state.
        let result = std::mem::take(&mut self.diversions[temp_idx]);
        // Remove the temporary diversion.
        self.diversions.pop();
        self.current_diversion = saved_diversion;
        result
    }

    /// Expand all arguments (POSIX m4 semantics: arguments are expanded
    /// before being passed to the macro).
    fn expand_args(&mut self, raw_args: &[String]) -> Vec<String> {
        raw_args
            .iter()
            .map(|arg| self.expand_to_string(arg))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Macro invocation
    // -----------------------------------------------------------------------

    /// Invoke a macro with the given arguments (already expanded).
    fn invoke_macro(&mut self, name: &str, def: &MacroDef, args: &[String]) {
        match def {
            MacroDef::User(body) => {
                let expanded = substitute_args(name, body, args);
                // Re-scan the result.
                self.expand_string(&expanded);
            }
            MacroDef::BuiltinDef(bi) => {
                self.invoke_builtin(*bi, name, args);
            }
        }
    }

    /// Dispatch a built-in macro call.
    #[allow(clippy::too_many_lines)]
    fn invoke_builtin(&mut self, bi: Builtin, name: &str, args: &[String]) {
        match bi {
            Builtin::Define => {
                let macro_name = args.first().map_or("", |s| s.as_str()).trim().to_string();
                let body = args.get(1).map_or("", |s| s.as_str()).to_string();
                if !macro_name.is_empty() {
                    self.macros
                        .entry(macro_name)
                        .or_default()
                        .push(MacroDef::User(body));
                }
            }
            Builtin::Undefine => {
                let macro_name = args.first().map_or("", |s| s.as_str()).trim();
                if !macro_name.is_empty() {
                    self.macros.remove(macro_name);
                }
            }
            Builtin::Defn => {
                let macro_name = args.first().map_or("", |s| s.as_str()).trim();
                if let Some(def) = self.lookup_macro(macro_name) {
                    match def {
                        MacroDef::User(body) => {
                            // Return the body wrapped in current quotes.
                            let qo = self.quote_open.clone();
                            let qc = self.quote_close.clone();
                            self.output(&qo);
                            self.output(&body);
                            self.output(&qc);
                        }
                        MacroDef::BuiltinDef(_) => {
                            // For builtins, just output the name quoted.
                            let qo = self.quote_open.clone();
                            let qc = self.quote_close.clone();
                            self.output(&qo);
                            self.output(macro_name);
                            self.output(&qc);
                        }
                    }
                }
            }
            Builtin::Ifdef => {
                let test_name = args.first().map_or("", |s| s.as_str()).trim();
                let if_def = args.get(1).map_or("", |s| s.as_str());
                let if_not_def = args.get(2).map_or("", |s| s.as_str());
                if self.macros.contains_key(test_name) {
                    self.expand_string(if_def);
                } else {
                    self.expand_string(if_not_def);
                }
            }
            Builtin::Ifelse => {
                self.builtin_ifelse(args);
            }
            Builtin::Shift => {
                // Return all arguments except the first, comma-separated.
                if args.len() > 1 {
                    let shifted: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
                    self.output(&shifted.join(","));
                }
            }
            Builtin::Changequote => {
                // Reset to the default ` ' quotes when called with no arguments
                // OR with an empty open-quote argument.  A bare `changequote()`
                // call yields a single empty-string argument (argc==1, $1==""),
                // not an empty arg list, so checking `args.is_empty()` alone
                // would fall through and set an empty open quote, silently
                // disabling all quoting.
                let open = args.first().map_or("", |s| s.as_str());
                if open.is_empty() {
                    self.quote_open = "`".to_string();
                    self.quote_close = "'".to_string();
                } else {
                    self.quote_open = open.to_string();
                    self.quote_close = args.get(1).map_or("'", |s| s.as_str()).to_string();
                }
            }
            Builtin::Changecom => {
                if args.is_empty() {
                    // Reset: disable comments.
                    self.comment_open = String::new();
                    self.comment_close = String::new();
                } else {
                    self.comment_open = args.first().map_or("", |s| s.as_str()).to_string();
                    self.comment_close = args.get(1).map_or("\n", |s| s.as_str()).to_string();
                }
            }
            Builtin::Dnl => {
                // dnl is handled specially in expand_string — but if it
                // appears as a normal call with args, we just discard
                // everything to the next newline.  In practice the scanner
                // handles this.  For robustness we do nothing extra here.
            }
            Builtin::Divert => {
                let n = args
                    .first()
                    .map_or(0, |s| s.trim().parse::<i32>().unwrap_or(0));
                if n >= 0 && (n as usize) < MAX_DIVERSIONS {
                    self.current_diversion = n;
                    while self.diversions.len() <= n as usize {
                        self.diversions.push(String::new());
                    }
                } else {
                    self.current_diversion = n; // negative = discard
                }
            }
            Builtin::Undivert => {
                if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
                    // Undivert all positive diversions into the current one.
                    let current = self.current_diversion;
                    for i in 1..self.diversions.len() {
                        if i as i32 != current {
                            let text = std::mem::take(&mut self.diversions[i]);
                            self.output(&text);
                        }
                    }
                } else {
                    for arg in args {
                        if let Ok(n) = arg.trim().parse::<usize>()
                            && n > 0
                                && n < self.diversions.len()
                                && n as i32 != self.current_diversion
                            {
                                let text = std::mem::take(&mut self.diversions[n]);
                                self.output(&text);
                            }
                    }
                }
            }
            Builtin::Divnum => {
                self.output(&self.current_diversion.to_string());
            }
            Builtin::Len => {
                let s = args.first().map_or("", |s| s.as_str());
                self.output(&s.len().to_string());
            }
            Builtin::Index => {
                let haystack = args.first().map_or("", |s| s.as_str());
                let needle = args.get(1).map_or("", |s| s.as_str());
                let idx = haystack.find(needle).map_or(-1, |i| i as i64);
                self.output(&idx.to_string());
            }
            Builtin::Substr => {
                let s = args.first().map_or("", |s| s.as_str());
                let start = args
                    .get(1)
                    .map_or(0i64, |v| v.trim().parse::<i64>().unwrap_or(0));
                let start = if start < 0 { 0usize } else { start as usize };
                if start >= s.len() {
                    // Empty result.
                } else if let Some(len_arg) = args.get(2) {
                    let sub_len = len_arg.trim().parse::<i64>().unwrap_or(0);
                    let sub_len = if sub_len < 0 {
                        0usize
                    } else {
                        sub_len as usize
                    };
                    let end = s.len().min(start + sub_len);
                    self.output(&s[start..end]);
                } else {
                    self.output(&s[start..]);
                }
            }
            Builtin::Translit => {
                let s = args.first().map_or("", |s| s.as_str());
                let from = args.get(1).map_or("", |s| s.as_str());
                let to = args.get(2).map_or("", |s| s.as_str());
                let result = translit(s, from, to);
                self.output(&result);
            }
            Builtin::Incr => {
                let n = args
                    .first()
                    .map_or(0i64, |s| s.trim().parse::<i64>().unwrap_or(0));
                self.output(&(n + 1).to_string());
            }
            Builtin::Decr => {
                let n = args
                    .first()
                    .map_or(0i64, |s| s.trim().parse::<i64>().unwrap_or(0));
                self.output(&(n - 1).to_string());
            }
            Builtin::Eval => {
                let expr = args.first().map_or("", |s| s.as_str()).trim();
                let radix = args
                    .get(1)
                    .map_or(10u32, |s| s.trim().parse::<u32>().unwrap_or(10));
                match eval_expr(expr) {
                    Ok(val) => {
                        self.output(&format_radix(val, radix));
                    }
                    Err(e) => {
                        self.warn(&format!("eval: {e}"));
                        self.output("0");
                    }
                }
            }
            Builtin::Syscmd => {
                let _cmd = args.first().map_or("", |s| s.as_str());
                // In the real OS we would execute via the shell.
                // For now, we set sysval to 127 (command not found).
                self.last_sysval = 127;
            }
            Builtin::Sysval => {
                self.output(&self.last_sysval.to_string());
            }
            Builtin::Maketemp => {
                let template = args.first().map_or("", |s| s.as_str());
                let result = self.maketemp(template);
                self.output(&result);
            }
            Builtin::Errprint => {
                for arg in args {
                    self.errprint(arg);
                }
            }
            Builtin::Dumpdef => {
                if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
                    // Dump all definitions.  Collect owned copies to avoid
                    // holding an immutable borrow on self.macros while we
                    // call self.dump_one_def (which borrows self mutably for
                    // stderr output).
                    let mut entries: Vec<(String, MacroDef)> = self
                        .macros
                        .iter()
                        .filter_map(|(k, stack)| stack.last().map(|d| (k.clone(), d.clone())))
                        .collect();
                    entries.sort_by(|a, b| a.0.cmp(&b.0));
                    for (name_key, def) in &entries {
                        self.dump_one_def(name_key, def);
                    }
                } else {
                    for arg in args {
                        let n = arg.trim();
                        if let Some(def) = self.lookup_macro(n) {
                            self.dump_one_def(n, &def);
                        } else {
                            self.errprint(&format!("m4: undefined macro `{n}'\n"));
                        }
                    }
                }
            }
            Builtin::Include => {
                let filename = args.first().map_or("", |s| s.as_str()).trim();
                if let Some(contents) = self.read_include(filename) {
                    self.expand_string(&contents);
                } else {
                    self.warn(&format!("{name}: cannot open `{filename}'"));
                }
            }
            Builtin::Sinclude => {
                let filename = args.first().map_or("", |s| s.as_str()).trim();
                if let Some(contents) = self.read_include(filename) {
                    self.expand_string(&contents);
                }
                // Silent on failure.
            }
            Builtin::Format => {
                let fmt_str = args.first().map_or("", |s| s.as_str());
                let rest = if args.len() > 1 { &args[1..] } else { &[] };
                let result = format_printf(fmt_str, rest);
                self.output(&result);
            }
            Builtin::Regexp => {
                let string = args.first().map_or("", |s| s.as_str());
                let pattern = args.get(1).map_or("", |s| s.as_str());
                let idx = simple_regex_match(string, pattern);
                self.output(&idx.to_string());
            }
            Builtin::Patsubst => {
                let string = args.first().map_or("", |s| s.as_str());
                let pattern = args.get(1).map_or("", |s| s.as_str());
                let replacement = args.get(2).map_or("", |s| s.as_str());
                let result = simple_regex_sub(string, pattern, replacement);
                self.output(&result);
            }
        }
    }

    /// Implement `ifelse(a, b, eq, ne)` with GNU chained extension:
    /// `ifelse(a, b, eq, c, d, eq2, ne2)` etc.
    fn builtin_ifelse(&mut self, args: &[String]) {
        let mut i = 0;
        loop {
            if i + 2 >= args.len() {
                // If we have exactly one leftover arg, output it as
                // the final default.
                if i < args.len() {
                    self.expand_string(&args[i]);
                }
                break;
            }
            let a = &args[i];
            let b = &args[i + 1];
            if a == b {
                if i + 2 < args.len() {
                    self.expand_string(&args[i + 2]);
                }
                break;
            }
            // Not equal: check if there is a fourth arg in this group.
            if i + 3 >= args.len() {
                break;
            }
            // Check for chained form: if we have 3+ more args after the
            // current group, treat them as another ifelse triplet.
            if i + 5 < args.len() {
                // More groups follow.
                i += 3;
                continue;
            }
            // Exactly one more arg = default.
            self.expand_string(&args[i + 3]);
            break;
        }
    }

    /// Read an include file, searching the include path.
    fn read_include(&self, filename: &str) -> Option<String> {
        // Try the filename as-is first.
        if let Ok(contents) = fs::read_to_string(filename) {
            return Some(contents);
        }
        // Search include directories.
        for dir in &self.include_dirs {
            let path = dir.join(filename);
            if let Ok(contents) = fs::read_to_string(&path) {
                return Some(contents);
            }
        }
        None
    }

    /// Dump a single macro definition to stderr.
    fn dump_one_def(&mut self, name: &str, def: &MacroDef) {
        match def {
            MacroDef::User(body) => {
                self.errprint(&format!("{name}:\t{body}\n"));
            }
            MacroDef::BuiltinDef(_) => {
                self.errprint(&format!("{name}:\t<{name}>\n"));
            }
        }
    }

    /// Generate a temporary filename from a template (replace XXXXXX).
    fn maketemp(&mut self, template: &str) -> String {
        self.temp_counter += 1;
        let replacement = format!("{:06}", self.temp_counter % 1_000_000);
        if let Some(idx) = template.find("XXXXXX") {
            let mut result = String::with_capacity(template.len());
            result.push_str(&template[..idx]);
            result.push_str(&replacement);
            result.push_str(&template[idx + 6..]);
            result
        } else {
            // No pattern — return as-is.
            template.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Argument substitution
// ---------------------------------------------------------------------------

/// Substitute `$0`..`$9`, `$#`, `$*`, `$@` in `body` using `args`.
fn substitute_args(name: &str, body: &str, args: &[String]) -> String {
    let chars: Vec<char> = body.chars().collect();
    let len = chars.len();
    let mut result = String::new();
    let mut i = 0;

    while i < len {
        if chars[i] == '$' && i + 1 < len {
            let next = chars[i + 1];
            match next {
                '0' => {
                    result.push_str(name);
                    i += 2;
                }
                '1'..='9' => {
                    let idx = (next as usize) - ('0' as usize);
                    if idx <= args.len()
                        && let Some(arg) = args.get(idx - 1) {
                            result.push_str(arg);
                        }
                    i += 2;
                }
                '#' => {
                    result.push_str(&args.len().to_string());
                    i += 2;
                }
                '*' => {
                    // All args, comma-separated, unquoted.
                    let joined: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    result.push_str(&joined.join(","));
                    i += 2;
                }
                '@' => {
                    // All args, each individually quoted.
                    for (j, arg) in args.iter().enumerate() {
                        if j > 0 {
                            result.push(',');
                        }
                        result.push('`');
                        result.push_str(arg);
                        result.push('\'');
                    }
                    i += 2;
                }
                _ => {
                    result.push('$');
                    i += 1;
                }
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Character classification
// ---------------------------------------------------------------------------

fn is_id_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_id_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

// ---------------------------------------------------------------------------
// Translit
// ---------------------------------------------------------------------------

/// Character transliteration: each character in `from` is mapped to the
/// corresponding character in `to`.  If `to` is shorter, excess `from`
/// characters are deleted.  Supports range notation `a-z`.
fn translit(s: &str, from: &str, to: &str) -> String {
    let from_chars = expand_ranges(from);
    let to_chars = expand_ranges(to);

    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if let Some(idx) = from_chars.iter().position(|&c| c == ch) {
            if idx < to_chars.len() {
                result.push(to_chars[idx]);
            }
            // else: character is deleted (to is shorter).
        } else {
            result.push(ch);
        }
    }
    result
}

/// Expand range notation like `a-z` into individual characters.
fn expand_ranges(spec: &str) -> Vec<char> {
    let chars: Vec<char> = spec.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i + 1] == '-' {
            let start = chars[i] as u32;
            let end = chars[i + 2] as u32;
            if start <= end {
                for code in start..=end {
                    if let Some(c) = char::from_u32(code) {
                        result.push(c);
                    }
                }
            } else {
                for code in (end..=start).rev() {
                    if let Some(c) = char::from_u32(code) {
                        result.push(c);
                    }
                }
            }
            i += 3;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Eval expression parser
// ---------------------------------------------------------------------------

/// Tokenize and evaluate an arithmetic expression.  Returns the result or
/// an error message.
fn eval_expr(expr: &str) -> Result<i64, String> {
    let tokens = tokenize_expr(expr)?;
    let mut parser = ExprParser::new(&tokens);
    let result = parser.parse_ternary()?;
    if parser.pos < parser.tokens.len() {
        return Err("unexpected token after expression".to_string());
    }
    Ok(result)
}

/// Expression tokens.
#[derive(Debug, Clone, PartialEq)]
enum ExprToken {
    Num(i64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Power, // **
    Eq,    // ==
    Ne,    // !=
    Lt,
    Gt,
    Le,       // <=
    Ge,       // >=
    And,      // &&
    Or,       // ||
    BitAnd,   // &
    BitOr,    // |
    BitXor,   // ^
    BitNot,   // ~
    Not,      // !
    Shl,      // <<
    Shr,      // >>
    Question, // ?
    Colon,    // :
    LParen,
    RParen,
}

/// Tokenize an expression string.
fn tokenize_expr(expr: &str) -> Result<Vec<ExprToken>, String> {
    let chars: Vec<char> = expr.chars().collect();
    let len = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Number: decimal, hex (0x), or octal (0...).
        if ch.is_ascii_digit() {
            let start = i;
            if ch == '0' && i + 1 < len && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                // Hexadecimal.
                i += 2;
                while i < len && chars[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let hex_str: String = chars[start + 2..i].iter().collect();
                let val = i64::from_str_radix(&hex_str, 16)
                    .map_err(|_| format!("invalid hex number: 0x{hex_str}"))?;
                tokens.push(ExprToken::Num(val));
            } else if ch == '0' && i + 1 < len && chars[i + 1].is_ascii_digit() {
                // Octal.
                i += 1;
                while i < len && chars[i] >= '0' && chars[i] <= '7' {
                    i += 1;
                }
                let oct_str: String = chars[start + 1..i].iter().collect();
                let val = i64::from_str_radix(&oct_str, 8)
                    .map_err(|_| format!("invalid octal number: 0{oct_str}"))?;
                tokens.push(ExprToken::Num(val));
            } else {
                // Decimal.
                while i < len && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let dec_str: String = chars[start..i].iter().collect();
                let val = dec_str
                    .parse::<i64>()
                    .map_err(|_| format!("invalid number: {dec_str}"))?;
                tokens.push(ExprToken::Num(val));
            }
            continue;
        }

        // Two-character operators.
        if i + 1 < len {
            let two: String = chars[i..i + 2].iter().collect();
            match two.as_str() {
                "**" => {
                    tokens.push(ExprToken::Power);
                    i += 2;
                    continue;
                }
                "==" => {
                    tokens.push(ExprToken::Eq);
                    i += 2;
                    continue;
                }
                "!=" => {
                    tokens.push(ExprToken::Ne);
                    i += 2;
                    continue;
                }
                "<=" => {
                    tokens.push(ExprToken::Le);
                    i += 2;
                    continue;
                }
                ">=" => {
                    tokens.push(ExprToken::Ge);
                    i += 2;
                    continue;
                }
                "&&" => {
                    tokens.push(ExprToken::And);
                    i += 2;
                    continue;
                }
                "||" => {
                    tokens.push(ExprToken::Or);
                    i += 2;
                    continue;
                }
                "<<" => {
                    tokens.push(ExprToken::Shl);
                    i += 2;
                    continue;
                }
                ">>" => {
                    tokens.push(ExprToken::Shr);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        // Single-character operators.
        match ch {
            '+' => tokens.push(ExprToken::Plus),
            '-' => tokens.push(ExprToken::Minus),
            '*' => tokens.push(ExprToken::Star),
            '/' => tokens.push(ExprToken::Slash),
            '%' => tokens.push(ExprToken::Percent),
            '<' => tokens.push(ExprToken::Lt),
            '>' => tokens.push(ExprToken::Gt),
            '&' => tokens.push(ExprToken::BitAnd),
            '|' => tokens.push(ExprToken::BitOr),
            '^' => tokens.push(ExprToken::BitXor),
            '~' => tokens.push(ExprToken::BitNot),
            '!' => tokens.push(ExprToken::Not),
            '?' => tokens.push(ExprToken::Question),
            ':' => tokens.push(ExprToken::Colon),
            '(' => tokens.push(ExprToken::LParen),
            ')' => tokens.push(ExprToken::RParen),
            _ => return Err(format!("unexpected character in expression: `{ch}'")),
        }
        i += 1;
    }

    Ok(tokens)
}

/// Recursive-descent expression parser with proper operator precedence.
struct ExprParser<'a> {
    tokens: &'a [ExprToken],
    pos: usize,
}

impl<'a> ExprParser<'a> {
    fn new(tokens: &'a [ExprToken]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&ExprToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&ExprToken> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &ExprToken) -> Result<(), String> {
        if self.peek() == Some(expected) {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {expected:?}"))
        }
    }

    // Precedence levels (lowest to highest):
    // 1.  ternary ? :
    // 2.  logical or ||
    // 3.  logical and &&
    // 4.  bitwise or |
    // 5.  bitwise xor ^
    // 6.  bitwise and &
    // 7.  equality == !=
    // 8.  relational < > <= >=
    // 9.  shift << >>
    // 10. additive + -
    // 11. multiplicative * / %
    // 12. power **
    // 13. unary + - ! ~

    fn parse_ternary(&mut self) -> Result<i64, String> {
        let cond = self.parse_or()?;
        if self.peek() == Some(&ExprToken::Question) {
            self.advance();
            let if_true = self.parse_ternary()?;
            self.expect(&ExprToken::Colon)?;
            let if_false = self.parse_ternary()?;
            Ok(if cond != 0 { if_true } else { if_false })
        } else {
            Ok(cond)
        }
    }

    fn parse_or(&mut self) -> Result<i64, String> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&ExprToken::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = i64::from(left != 0 || right != 0);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<i64, String> {
        let mut left = self.parse_bitor()?;
        while self.peek() == Some(&ExprToken::And) {
            self.advance();
            let right = self.parse_bitor()?;
            left = i64::from(left != 0 && right != 0);
        }
        Ok(left)
    }

    fn parse_bitor(&mut self) -> Result<i64, String> {
        let mut left = self.parse_bitxor()?;
        while self.peek() == Some(&ExprToken::BitOr) {
            self.advance();
            let right = self.parse_bitxor()?;
            left |= right;
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> Result<i64, String> {
        let mut left = self.parse_bitand()?;
        while self.peek() == Some(&ExprToken::BitXor) {
            self.advance();
            let right = self.parse_bitand()?;
            left ^= right;
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> Result<i64, String> {
        let mut left = self.parse_equality()?;
        while self.peek() == Some(&ExprToken::BitAnd) {
            self.advance();
            let right = self.parse_equality()?;
            left &= right;
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<i64, String> {
        let mut left = self.parse_relational()?;
        loop {
            match self.peek() {
                Some(&ExprToken::Eq) => {
                    self.advance();
                    let right = self.parse_relational()?;
                    left = i64::from(left == right);
                }
                Some(&ExprToken::Ne) => {
                    self.advance();
                    let right = self.parse_relational()?;
                    left = i64::from(left != right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> Result<i64, String> {
        let mut left = self.parse_shift()?;
        loop {
            match self.peek() {
                Some(&ExprToken::Lt) => {
                    self.advance();
                    let right = self.parse_shift()?;
                    left = i64::from(left < right);
                }
                Some(&ExprToken::Gt) => {
                    self.advance();
                    let right = self.parse_shift()?;
                    left = i64::from(left > right);
                }
                Some(&ExprToken::Le) => {
                    self.advance();
                    let right = self.parse_shift()?;
                    left = i64::from(left <= right);
                }
                Some(&ExprToken::Ge) => {
                    self.advance();
                    let right = self.parse_shift()?;
                    left = i64::from(left >= right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<i64, String> {
        let mut left = self.parse_additive()?;
        loop {
            match self.peek() {
                Some(&ExprToken::Shl) => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = left.wrapping_shl(right as u32);
                }
                Some(&ExprToken::Shr) => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = left.wrapping_shr(right as u32);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<i64, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            match self.peek() {
                Some(&ExprToken::Plus) => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = left.wrapping_add(right);
                }
                Some(&ExprToken::Minus) => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = left.wrapping_sub(right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<i64, String> {
        let mut left = self.parse_power()?;
        loop {
            match self.peek() {
                Some(&ExprToken::Star) => {
                    self.advance();
                    let right = self.parse_power()?;
                    left = left.wrapping_mul(right);
                }
                Some(&ExprToken::Slash) => {
                    self.advance();
                    let right = self.parse_power()?;
                    if right == 0 {
                        return Err("division by zero".to_string());
                    }
                    left = left.wrapping_div(right);
                }
                Some(&ExprToken::Percent) => {
                    self.advance();
                    let right = self.parse_power()?;
                    if right == 0 {
                        return Err("modulo by zero".to_string());
                    }
                    left = left.wrapping_rem(right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<i64, String> {
        let base = self.parse_unary()?;
        if self.peek() == Some(&ExprToken::Power) {
            self.advance();
            // Right-associative.
            let exp = self.parse_power()?;
            Ok(int_pow(base, exp))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<i64, String> {
        match self.peek() {
            Some(&ExprToken::Plus) => {
                self.advance();
                self.parse_unary()
            }
            Some(&ExprToken::Minus) => {
                self.advance();
                let val = self.parse_unary()?;
                Ok(val.wrapping_neg())
            }
            Some(&ExprToken::Not) => {
                self.advance();
                let val = self.parse_unary()?;
                Ok(i64::from(val == 0))
            }
            Some(&ExprToken::BitNot) => {
                self.advance();
                let val = self.parse_unary()?;
                Ok(!val)
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<i64, String> {
        match self.peek() {
            Some(&ExprToken::Num(n)) => {
                self.advance();
                Ok(n)
            }
            Some(&ExprToken::LParen) => {
                self.advance();
                let val = self.parse_ternary()?;
                self.expect(&ExprToken::RParen)?;
                Ok(val)
            }
            _ => Err("expected number or `('".to_string()),
        }
    }
}

/// Integer exponentiation.
fn int_pow(mut base: i64, mut exp: i64) -> i64 {
    if exp < 0 {
        return 0; // Integer division result of 1/base^|exp| = 0.
    }
    let mut result: i64 = 1;
    while exp > 0 {
        if exp & 1 != 0 {
            result = result.wrapping_mul(base);
        }
        exp >>= 1;
        if exp > 0 {
            base = base.wrapping_mul(base);
        }
    }
    result
}

/// Format an integer in the given radix (2..36).
fn format_radix(val: i64, radix: u32) -> String {
    if !(2..=36).contains(&radix) {
        return val.to_string();
    }
    if radix == 10 {
        return val.to_string();
    }
    let negative = val < 0;
    let mut n = if negative {
        (val as i128).unsigned_abs()
    } else {
        val as u128
    };
    if n == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    let rad = radix as u128;
    while n > 0 {
        let d = (n % rad) as u32;
        let ch = if d < 10 {
            (b'0' + d as u8) as char
        } else {
            (b'a' + (d - 10) as u8) as char
        };
        digits.push(ch);
        n /= rad;
    }
    if negative {
        digits.push('-');
    }
    digits.reverse();
    digits.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Printf-style formatting
// ---------------------------------------------------------------------------

/// A simplified printf implementation supporting `%d`, `%s`, `%x`, `%o`, `%c`,
/// `%%`, and width/precision modifiers.
fn format_printf(fmt: &str, args: &[String]) -> String {
    let chars: Vec<char> = fmt.chars().collect();
    let len = chars.len();
    let mut result = String::new();
    let mut i = 0;
    let mut arg_idx = 0;

    while i < len {
        if chars[i] == '%' {
            i += 1;
            if i >= len {
                break;
            }
            if chars[i] == '%' {
                result.push('%');
                i += 1;
                continue;
            }

            // Parse flags.
            let mut left_align = false;
            let mut zero_pad = false;
            while i < len {
                match chars[i] {
                    '-' => left_align = true,
                    '0' => zero_pad = true,
                    _ => break,
                }
                i += 1;
            }

            // Parse width.
            let mut width = 0usize;
            while i < len && chars[i].is_ascii_digit() {
                width = width * 10 + (chars[i] as usize - '0' as usize);
                i += 1;
            }

            // Parse precision.
            let mut precision: Option<usize> = None;
            if i < len && chars[i] == '.' {
                i += 1;
                let mut prec = 0usize;
                while i < len && chars[i].is_ascii_digit() {
                    prec = prec * 10 + (chars[i] as usize - '0' as usize);
                    i += 1;
                }
                precision = Some(prec);
            }

            if i >= len {
                break;
            }

            let spec = chars[i];
            i += 1;
            let arg_val = args.get(arg_idx).map(|s| s.as_str()).unwrap_or("");
            arg_idx += 1;

            let formatted = match spec {
                'd' | 'i' => {
                    let n: i64 = arg_val.trim().parse().unwrap_or(0);
                    format_with_width(&n.to_string(), width, left_align, zero_pad)
                }
                's' => {
                    let s = if let Some(prec) = precision {
                        if prec < arg_val.len() {
                            &arg_val[..prec]
                        } else {
                            arg_val
                        }
                    } else {
                        arg_val
                    };
                    format_with_width(s, width, left_align, false)
                }
                'x' => {
                    let n: i64 = arg_val.trim().parse().unwrap_or(0);
                    let hex = format!("{:x}", n);
                    format_with_width(&hex, width, left_align, zero_pad)
                }
                'X' => {
                    let n: i64 = arg_val.trim().parse().unwrap_or(0);
                    let hex = format!("{:X}", n);
                    format_with_width(&hex, width, left_align, zero_pad)
                }
                'o' => {
                    let n: i64 = arg_val.trim().parse().unwrap_or(0);
                    let oct = format!("{:o}", n);
                    format_with_width(&oct, width, left_align, zero_pad)
                }
                'c' => {
                    let ch = arg_val.chars().next().unwrap_or('\0');
                    format_with_width(&ch.to_string(), width, left_align, false)
                }
                _ => {
                    // Unknown spec — pass through.
                    format!("%{spec}")
                }
            };
            result.push_str(&formatted);
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Pad a string to the given width.
fn format_with_width(s: &str, width: usize, left_align: bool, zero_pad: bool) -> String {
    if s.len() >= width {
        return s.to_string();
    }
    let pad_char = if zero_pad { '0' } else { ' ' };
    let padding = width - s.len();
    if left_align {
        format!("{s}{}", " ".repeat(padding))
    } else {
        let pad: String = std::iter::repeat_n(pad_char, padding).collect();
        format!("{pad}{s}")
    }
}

// ---------------------------------------------------------------------------
// Simple regex (literal substring match for basic patterns)
// ---------------------------------------------------------------------------

/// Basic regex match: find `pattern` in `string`.  Returns the position of
/// the first match, or -1 if not found.
///
/// Supports only literal strings and the anchors `^` and `$`.  Full regex
/// would require a proper engine; this covers the most common m4 usage.
fn simple_regex_match(string: &str, pattern: &str) -> i64 {
    if pattern.is_empty() {
        return 0;
    }

    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$') && !pattern.ends_with("\\$");

    let pat = if anchored_start && anchored_end && pattern.len() >= 2 {
        &pattern[1..pattern.len() - 1]
    } else if anchored_start {
        &pattern[1..]
    } else if anchored_end {
        &pattern[..pattern.len() - 1]
    } else {
        pattern
    };

    // Handle `.` as wildcard (single char).
    if pat.contains('.') || pat.contains('*') || pat.contains('+') || pat.contains('?') {
        // Fall back to a very simple NFA-like approach for basic patterns.
        return simple_pattern_search(string, pattern);
    }

    if anchored_start && anchored_end {
        if string == pat { 0 } else { -1 }
    } else if anchored_start {
        if string.starts_with(pat) { 0 } else { -1 }
    } else if anchored_end {
        if string.ends_with(pat) {
            (string.len() - pat.len()) as i64
        } else {
            -1
        }
    } else {
        string.find(pat).map_or(-1, |i| i as i64)
    }
}

/// Very basic pattern matching for `.` (any char), `*` (zero or more of
/// previous), and literal characters.  Used by `regexp` and `patsubst`.
fn simple_pattern_search(string: &str, pattern: &str) -> i64 {
    let anchored_start = pattern.starts_with('^');
    let pat = if anchored_start {
        &pattern[1..]
    } else {
        pattern
    };

    let string_chars: Vec<char> = string.chars().collect();
    let slen = string_chars.len();

    if anchored_start {
        if simple_match_at(&string_chars, 0, pat) {
            0
        } else {
            -1
        }
    } else {
        for start in 0..=slen {
            if simple_match_at(&string_chars, start, pat) {
                return start as i64;
            }
        }
        -1
    }
}

/// Try to match `pattern` at position `start` in `string_chars`.
fn simple_match_at(string_chars: &[char], start: usize, pattern: &str) -> bool {
    let pat_chars: Vec<char> = pattern.chars().collect();
    match_recursive(string_chars, start, &pat_chars, 0)
}

/// Recursive pattern matcher.
fn match_recursive(s: &[char], mut si: usize, p: &[char], mut pi: usize) -> bool {
    while pi < p.len() {
        // Check for `$` anchor at end.
        if p[pi] == '$' && pi + 1 == p.len() {
            return si == s.len();
        }

        // Check for `X*` pattern (char/dot followed by star).
        if pi + 1 < p.len() && p[pi + 1] == '*' {
            let pat_char = p[pi];
            // Try matching zero or more of `pat_char`.
            // Greedy: try longest match first.
            let mut end = si;
            while end < s.len() && char_matches(s[end], pat_char) {
                end += 1;
            }
            // Try from longest to shortest.
            let mut try_len = end;
            loop {
                if match_recursive(s, try_len, p, pi + 2) {
                    return true;
                }
                if try_len == si {
                    break;
                }
                try_len -= 1;
            }
            return false;
        }

        // Check for `X+` pattern.
        if pi + 1 < p.len() && p[pi + 1] == '+' {
            let pat_char = p[pi];
            if si >= s.len() || !char_matches(s[si], pat_char) {
                return false;
            }
            si += 1;
            // Now it behaves like `X*` for the rest.
            let mut end = si;
            while end < s.len() && char_matches(s[end], pat_char) {
                end += 1;
            }
            let mut try_len = end;
            loop {
                if match_recursive(s, try_len, p, pi + 2) {
                    return true;
                }
                if try_len == si {
                    break;
                }
                try_len -= 1;
            }
            return false;
        }

        // Check for `X?` pattern.
        if pi + 1 < p.len() && p[pi + 1] == '?' {
            let pat_char = p[pi];
            // Try with one match.
            if si < s.len() && char_matches(s[si], pat_char)
                && match_recursive(s, si + 1, p, pi + 2) {
                    return true;
                }
            // Try with zero matches.
            return match_recursive(s, si, p, pi + 2);
        }

        // Escaped character.
        if p[pi] == '\\' && pi + 1 < p.len() {
            pi += 1;
            if si >= s.len() || s[si] != p[pi] {
                return false;
            }
            si += 1;
            pi += 1;
            continue;
        }

        // Regular character or `.`.
        if si >= s.len() {
            return false;
        }
        if !char_matches(s[si], p[pi]) {
            return false;
        }
        si += 1;
        pi += 1;
    }
    true
}

/// Does character `c` match pattern char `p`?  `.` matches anything.
fn char_matches(c: char, p: char) -> bool {
    p == '.' || c == p
}

/// Regex substitution: replace first match of `pattern` in `string` with
/// `replacement`.  `&` in replacement = matched text.
fn simple_regex_sub(string: &str, pattern: &str, replacement: &str) -> String {
    if pattern.is_empty() {
        // Empty pattern: prepend replacement to each character gap.
        let mut result = String::new();
        for ch in string.chars() {
            result.push_str(replacement);
            result.push(ch);
        }
        result.push_str(replacement);
        return result;
    }

    let string_chars: Vec<char> = string.chars().collect();
    let slen = string_chars.len();

    let anchored_start = pattern.starts_with('^');

    let mut result = String::new();
    let mut pos = 0;
    let mut did_replace = false;

    while pos <= slen {
        if (!did_replace || !anchored_start)
            && let Some(match_len) = find_match_len(&string_chars, pos, pattern) {
                // Found a match at `pos` of length `match_len`.
                let matched: String = string_chars[pos..pos + match_len].iter().collect();
                // Build replacement, substituting `&` for matched text.
                for ch in replacement.chars() {
                    if ch == '&' {
                        result.push_str(&matched);
                    } else {
                        result.push(ch);
                    }
                }
                pos += if match_len > 0 { match_len } else { 1 };
                did_replace = true;
                if anchored_start {
                    // Only replace once for anchored patterns.
                    while pos < slen {
                        result.push(string_chars[pos]);
                        pos += 1;
                    }
                    return result;
                }
                continue;
            }
        if pos < slen {
            result.push(string_chars[pos]);
        }
        pos += 1;
    }

    result
}

/// Find the length of match at `start` for the given pattern.
fn find_match_len(s: &[char], start: usize, pattern: &str) -> Option<usize> {
    let anchored_start = pattern.starts_with('^');
    let pat = if anchored_start {
        &pattern[1..]
    } else {
        pattern
    };

    if anchored_start && start != 0 {
        return None;
    }

    let pat_chars: Vec<char> = pat.chars().collect();

    // Try increasing match lengths.
    for end in start..=s.len() {
        if match_exact(s, start, end, &pat_chars) {
            // Find longest match.
            let mut best = end;
            for longer_end in (end + 1)..=s.len() {
                if match_exact(s, start, longer_end, &pat_chars) {
                    best = longer_end;
                } else {
                    break;
                }
            }
            return Some(best - start);
        }
    }
    None
}

/// Check if `s[start..end]` exactly matches the pattern.
fn match_exact(s: &[char], start: usize, end: usize, pat_chars: &[char]) -> bool {
    // Check if the pattern can match s[start..end] consuming exactly
    // those characters.
    match_exact_recursive(s, start, end, pat_chars, 0)
}

fn match_exact_recursive(s: &[char], si: usize, end: usize, p: &[char], pi: usize) -> bool {
    if pi == p.len() {
        return si == end;
    }

    // Handle `$` anchor.
    if p[pi] == '$' && pi + 1 == p.len() {
        return si == end && end == s.len();
    }

    // Handle quantifiers.
    if pi + 1 < p.len() && p[pi + 1] == '*' {
        let pat_ch = p[pi];
        let mut cur = si;
        // Try zero matches first (non-greedy for exact matching).
        if match_exact_recursive(s, cur, end, p, pi + 2) {
            return true;
        }
        while cur < end && char_matches(s[cur], pat_ch) {
            cur += 1;
            if match_exact_recursive(s, cur, end, p, pi + 2) {
                return true;
            }
        }
        return false;
    }

    if pi + 1 < p.len() && p[pi + 1] == '+' {
        let pat_ch = p[pi];
        if si >= end || !char_matches(s[si], pat_ch) {
            return false;
        }
        let mut cur = si + 1;
        if match_exact_recursive(s, cur, end, p, pi + 2) {
            return true;
        }
        while cur < end && char_matches(s[cur], pat_ch) {
            cur += 1;
            if match_exact_recursive(s, cur, end, p, pi + 2) {
                return true;
            }
        }
        return false;
    }

    if pi + 1 < p.len() && p[pi + 1] == '?' {
        let pat_ch = p[pi];
        if match_exact_recursive(s, si, end, p, pi + 2) {
            return true;
        }
        if si < end && char_matches(s[si], pat_ch) {
            return match_exact_recursive(s, si + 1, end, p, pi + 2);
        }
        return false;
    }

    // Escaped character.
    if p[pi] == '\\' && pi + 1 < p.len() {
        if si < end && s[si] == p[pi + 1] {
            return match_exact_recursive(s, si + 1, end, p, pi + 2);
        }
        return false;
    }

    // Single character.
    if si < end && char_matches(s[si], p[pi]) {
        return match_exact_recursive(s, si + 1, end, p, pi + 1);
    }

    false
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let opts = parse_args(&args[1..]);

    let mut proc = if opts.prefix_builtins {
        Processor::new_with_prefix()
    } else {
        Processor::new()
    };

    proc.sync_lines = opts.sync_lines;
    proc.quiet = opts.quiet;
    proc.include_dirs = opts.include_dirs.clone();

    // Apply -D definitions.
    for (name, value) in &opts.defines {
        proc.macros
            .entry(name.clone())
            .or_default()
            .push(MacroDef::User(value.clone()));
    }

    // Apply -U undefinitions.
    for name in &opts.undefines {
        proc.macros.remove(name);
    }

    // Read input.
    let input = if opts.input_files.is_empty() {
        let mut buf = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut buf) {
            eprintln!("m4: error reading stdin: {e}");
            process::exit(1);
        }
        buf
    } else {
        let mut combined = String::new();
        for filename in &opts.input_files {
            if filename == "-" {
                let mut buf = String::new();
                if let Err(e) = io::stdin().read_to_string(&mut buf) {
                    eprintln!("m4: error reading stdin: {e}");
                    process::exit(1);
                }
                combined.push_str(&buf);
            } else {
                match fs::read_to_string(filename) {
                    Ok(contents) => combined.push_str(&contents),
                    Err(e) => {
                        eprintln!("m4: cannot open `{filename}': {e}");
                        process::exit(1);
                    }
                }
            }
        }
        combined
    };

    let output = proc.process(&input);

    if let Err(e) = io::stdout().write_all(output.as_bytes()) {
        eprintln!("m4: write error: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a fresh processor, expand input, return output.
    fn run(input: &str) -> String {
        let mut p = Processor::new();
        p.process(input)
    }

    /// Helper: create processor, expand, return (output, stderr).
    fn run_with_stderr(input: &str) -> (String, String) {
        let mut p = Processor::new();
        let out = p.process(input);
        (out, p.stderr_buf.clone())
    }

    // -- Token scanning --

    #[test]
    fn test_plain_text() {
        assert_eq!(run("hello world"), "hello world");
    }

    #[test]
    fn test_identifier_passthrough() {
        // `foo` is not a defined macro, so it passes through.
        assert_eq!(run("foo bar"), "foo bar");
    }

    #[test]
    fn test_quoted_string() {
        assert_eq!(run("`hello'"), "hello");
    }

    #[test]
    fn test_nested_quotes() {
        assert_eq!(run("``hello''"), "`hello'");
    }

    #[test]
    fn test_deeply_nested_quotes() {
        assert_eq!(run("```hello'''"), "``hello''");
    }

    #[test]
    fn test_comment_passthrough() {
        // Comments pass through by default.
        assert_eq!(run("hello # comment\nworld"), "hello # comment\nworld");
    }

    // -- define / undefine --

    #[test]
    fn test_define_simple() {
        assert_eq!(run("define(`foo', `bar')foo"), "bar");
    }

    #[test]
    fn test_define_with_args() {
        assert_eq!(
            run("define(`greet', `hello $1')greet(`world')"),
            "hello world"
        );
    }

    #[test]
    fn test_define_multiple_args() {
        assert_eq!(run("define(`pair', `($1, $2)')pair(`a', `b')"), "(a, b)");
    }

    #[test]
    fn test_define_dollar_zero() {
        // $0 expands to the macro name.  We test the substitution
        // directly on the substitute_args function to avoid re-scan
        // complications.
        let result = substitute_args("mymacro", "name=$0", &[]);
        assert_eq!(result, "name=mymacro");
    }

    #[test]
    fn test_define_dollar_hash() {
        assert_eq!(run("define(`count', `$#')count(`a', `b', `c')"), "3");
    }

    #[test]
    fn test_define_dollar_star() {
        assert_eq!(run("define(`all', `$*')all(`a', `b', `c')"), "a,b,c");
    }

    #[test]
    fn test_define_dollar_at() {
        assert_eq!(
            run("define(`allq', `$@')allq(`a', `b')"),
            "a,b" // $@ quotes get stripped on rescan.
        );
    }

    #[test]
    fn test_undefine() {
        assert_eq!(run("define(`foo', `bar')undefine(`foo')foo"), "foo");
    }

    #[test]
    fn test_redefine() {
        assert_eq!(run("define(`x', `1')define(`x', `2')x"), "2");
    }

    // -- defn --

    #[test]
    fn test_defn() {
        assert_eq!(
            run("define(`foo', `hello')define(`bar', defn(`foo'))bar"),
            "hello"
        );
    }

    // -- ifdef --

    #[test]
    fn test_ifdef_debug() {
        // Debug: check what define does to the macro table.
        let mut p = Processor::new();
        let out = p.process("define(`foo', `1')");
        assert_eq!(out, "", "define should produce no output");
        assert!(
            p.macros.contains_key("foo"),
            "foo should be defined after define()"
        );
    }

    #[test]
    fn test_ifdef_debug2() {
        // Debug: check combined define + ifdef.
        let mut p = Processor::new();
        // First process define.
        p.expand_string("define(`foo', `1')");
        assert!(
            p.macros.contains_key("foo"),
            "foo should be defined after expand_string"
        );

        // Manually collect and expand args to see what happens.
        let input = "ifdef(`foo', `yes', `no')";
        let chars: Vec<char> = input.chars().collect();
        let mut pos = 0;
        // Skip "ifdef".
        while pos < chars.len() && super::is_id_continue(chars[pos]) {
            pos += 1;
        }
        let raw_args = p.collect_args(&chars, &mut pos);
        let expanded = p.expand_args(&raw_args);

        // Check foo still exists after arg expansion.
        assert!(
            p.macros.contains_key("foo"),
            "foo should STILL be defined after expand_args. expanded={expanded:?}"
        );

        assert_eq!(expanded[0], "foo", "first arg should be 'foo'");
        assert_eq!(expanded[1], "yes", "second arg should be 'yes'");
        assert_eq!(expanded[2], "no", "third arg should be 'no'");
    }

    #[test]
    fn test_ifdef_defined() {
        assert_eq!(run("define(`foo', `1')ifdef(`foo', `yes', `no')"), "yes");
    }

    #[test]
    fn test_ifdef_undefined() {
        assert_eq!(run("ifdef(`foo', `yes', `no')"), "no");
    }

    #[test]
    fn test_ifdef_no_else() {
        assert_eq!(run("ifdef(`foo', `yes')"), "");
    }

    // -- ifelse --

    #[test]
    fn test_ifelse_equal() {
        assert_eq!(run("ifelse(`a', `a', `yes', `no')"), "yes");
    }

    #[test]
    fn test_ifelse_not_equal() {
        assert_eq!(run("ifelse(`a', `b', `yes', `no')"), "no");
    }

    #[test]
    fn test_ifelse_chained() {
        assert_eq!(run("ifelse(`a', `b', `1', `a', `a', `2', `3')"), "2");
    }

    #[test]
    fn test_ifelse_chained_default() {
        assert_eq!(
            run("ifelse(`a', `b', `1', `c', `d', `2', `default')"),
            "default"
        );
    }

    // -- shift --

    #[test]
    fn test_shift() {
        assert_eq!(run("shift(`a', `b', `c')"), "b,c");
    }

    #[test]
    fn test_shift_single() {
        assert_eq!(run("shift(`a')"), "");
    }

    // -- len --

    #[test]
    fn test_len() {
        assert_eq!(run("len(`hello')"), "5");
    }

    #[test]
    fn test_len_empty() {
        assert_eq!(run("len(`')"), "0");
    }

    // -- index --

    #[test]
    fn test_index_found() {
        assert_eq!(run("index(`hello world', `world')"), "6");
    }

    #[test]
    fn test_index_not_found() {
        assert_eq!(run("index(`hello', `xyz')"), "-1");
    }

    // -- substr --

    #[test]
    fn test_substr() {
        assert_eq!(run("substr(`hello world', `6')"), "world");
    }

    #[test]
    fn test_substr_with_len() {
        assert_eq!(run("substr(`hello world', `0', `5')"), "hello");
    }

    #[test]
    fn test_substr_empty() {
        assert_eq!(run("substr(`hello', `10')"), "");
    }

    // -- translit --

    #[test]
    fn test_translit_simple() {
        assert_eq!(run("translit(`hello', `elo', `ELO')"), "hELLO");
    }

    #[test]
    fn test_translit_delete() {
        assert_eq!(run("translit(`hello', `l')"), "heo");
    }

    #[test]
    fn test_translit_range() {
        assert_eq!(run("translit(`hello', `a-z', `A-Z')"), "HELLO");
    }

    // -- incr / decr --

    #[test]
    fn test_incr() {
        assert_eq!(run("incr(`5')"), "6");
    }

    #[test]
    fn test_decr() {
        assert_eq!(run("decr(`5')"), "4");
    }

    #[test]
    fn test_incr_negative() {
        assert_eq!(run("incr(`-1')"), "0");
    }

    // -- eval --

    #[test]
    fn test_eval_simple() {
        assert_eq!(run("eval(`2 + 3')"), "5");
    }

    #[test]
    fn test_eval_multiply() {
        assert_eq!(run("eval(`6 * 7')"), "42");
    }

    #[test]
    fn test_eval_division() {
        assert_eq!(run("eval(`10 / 3')"), "3");
    }

    #[test]
    fn test_eval_modulo() {
        assert_eq!(run("eval(`10 % 3')"), "1");
    }

    #[test]
    fn test_eval_power() {
        assert_eq!(run("eval(`2 ** 10')"), "1024");
    }

    #[test]
    fn test_eval_negative() {
        assert_eq!(run("eval(`-5 + 3')"), "-2");
    }

    #[test]
    fn test_eval_comparison() {
        assert_eq!(run("eval(`3 > 2')"), "1");
        assert_eq!(run("eval(`2 > 3')"), "0");
    }

    #[test]
    fn test_eval_equality() {
        assert_eq!(run("eval(`5 == 5')"), "1");
        assert_eq!(run("eval(`5 != 5')"), "0");
    }

    #[test]
    fn test_eval_logical() {
        assert_eq!(run("eval(`1 && 1')"), "1");
        assert_eq!(run("eval(`1 && 0')"), "0");
        assert_eq!(run("eval(`0 || 1')"), "1");
    }

    #[test]
    fn test_eval_bitwise() {
        assert_eq!(run("eval(`5 & 3')"), "1");
        assert_eq!(run("eval(`5 | 3')"), "7");
        assert_eq!(run("eval(`5 ^ 3')"), "6");
    }

    #[test]
    fn test_eval_shift() {
        assert_eq!(run("eval(`1 << 4')"), "16");
        assert_eq!(run("eval(`16 >> 2')"), "4");
    }

    #[test]
    fn test_eval_parentheses() {
        assert_eq!(run("eval(`(2 + 3) * 4')"), "20");
    }

    #[test]
    fn test_eval_ternary() {
        assert_eq!(run("eval(`1 ? 42 : 99')"), "42");
        assert_eq!(run("eval(`0 ? 42 : 99')"), "99");
    }

    #[test]
    fn test_eval_hex() {
        assert_eq!(run("eval(`0xff')"), "255");
    }

    #[test]
    fn test_eval_octal() {
        assert_eq!(run("eval(`010')"), "8");
    }

    #[test]
    fn test_eval_not() {
        assert_eq!(run("eval(`!0')"), "1");
        assert_eq!(run("eval(`!1')"), "0");
    }

    #[test]
    fn test_eval_bitnot() {
        // ~0 in two's complement 64-bit = -1.
        assert_eq!(run("eval(`~0')"), "-1");
    }

    #[test]
    fn test_eval_radix() {
        // eval(255, 16) should output ff.
        let mut p = Processor::new();
        // We need to define a macro that calls eval with radix 16.
        let out = p.process("eval(`255', `16')");
        assert_eq!(out, "ff");
    }

    #[test]
    fn test_eval_precedence() {
        // Multiplication before addition.
        assert_eq!(run("eval(`2 + 3 * 4')"), "14");
    }

    #[test]
    fn test_eval_complex() {
        assert_eq!(run("eval(`(1 + 2) * (3 + 4)')"), "21");
    }

    // -- divert / undivert / divnum --

    #[test]
    fn test_divert_basic() {
        assert_eq!(run("divert(`1')hello divert(`0')world"), "worldhello ");
    }

    #[test]
    fn test_divert_discard() {
        assert_eq!(run("divert(`-1')discarded divert(`0')kept"), "kept");
    }

    #[test]
    fn test_divnum() {
        assert_eq!(run("divnum"), "0");
    }

    #[test]
    fn test_divnum_after_divert() {
        assert_eq!(run("divert(`3')divnum"), "3");
    }

    #[test]
    fn test_undivert_specific() {
        let mut p = Processor::new();
        // Divert to 1, write some text, switch back, then undivert 1.
        let out = p.process("divert(`1')buffered divert(`0')main undivert(`1')");
        assert_eq!(out, "main buffered ");
    }

    // -- changequote --

    #[test]
    fn test_changequote() {
        assert_eq!(run("changequote(`[', `]')define([foo], [bar])foo"), "bar");
    }

    #[test]
    fn test_changequote_reset() {
        assert_eq!(
            run("changequote(`[', `]')changequote()define(`foo', `bar')foo"),
            "bar"
        );
    }

    // -- changecom --

    #[test]
    fn test_changecom() {
        // After changecom, default # comments are disabled.
        let out = run("changecom(`//', `\n')define(`x', `1')x // comment\n");
        assert!(out.contains("1"));
        assert!(out.contains("// comment"));
    }

    // -- dnl --

    #[test]
    fn test_dnl() {
        assert_eq!(run("define(`foo', `bar')dnl this is discarded\nfoo"), "bar");
    }

    #[test]
    fn test_dnl_multiple() {
        assert_eq!(run("define(`a', `1')dnl\ndefine(`b', `2')dnl\na b"), "1 2");
    }

    // -- include / sinclude --

    #[test]
    fn test_sinclude_missing() {
        // sinclude of a nonexistent file produces no output and no error.
        assert_eq!(run("sinclude(`nonexistent_file_12345.m4')"), "");
    }

    // -- errprint --

    #[test]
    fn test_errprint() {
        let (out, err) = run_with_stderr("errprint(`hello stderr')");
        assert_eq!(out, "");
        assert!(err.contains("hello stderr"));
    }

    // -- dumpdef --

    #[test]
    fn test_dumpdef() {
        let (_, err) = run_with_stderr("define(`foo', `bar')dumpdef(`foo')");
        assert!(err.contains("foo:"));
        assert!(err.contains("bar"));
    }

    // -- sysval --

    #[test]
    fn test_sysval_initial() {
        assert_eq!(run("sysval"), "0");
    }

    // -- maketemp --

    #[test]
    fn test_maketemp() {
        let out = run("maketemp(`/tmp/fileXXXXXX')");
        assert!(out.starts_with("/tmp/file"));
        assert!(!out.contains("XXXXXX"));
    }

    // -- format --

    #[test]
    fn test_format_string() {
        assert_eq!(run("format(`hello %s', `world')"), "hello world");
    }

    #[test]
    fn test_format_integer() {
        assert_eq!(run("format(`num=%d', `42')"), "num=42");
    }

    #[test]
    fn test_format_hex() {
        assert_eq!(run("format(`hex=%x', `255')"), "hex=ff");
    }

    #[test]
    fn test_format_width() {
        assert_eq!(run("format(`[%10s]', `hi')"), "[        hi]");
    }

    #[test]
    fn test_format_percent() {
        assert_eq!(run("format(`100%%')"), "100%");
    }

    // -- regexp --

    #[test]
    fn test_regexp_found() {
        assert_eq!(run("regexp(`hello world', `world')"), "6");
    }

    #[test]
    fn test_regexp_not_found() {
        assert_eq!(run("regexp(`hello', `xyz')"), "-1");
    }

    #[test]
    fn test_regexp_anchored() {
        assert_eq!(run("regexp(`hello', `^hello$')"), "0");
    }

    // -- patsubst --

    #[test]
    fn test_patsubst() {
        assert_eq!(
            run("patsubst(`hello world', `world', `earth')"),
            "hello earth"
        );
    }

    // -- Nested macro expansion --

    #[test]
    fn test_nested_expansion() {
        assert_eq!(run("define(`a', `b')define(`b', `c')a"), "c");
    }

    #[test]
    fn test_nested_args() {
        assert_eq!(
            run("define(`wrap', `($1)')define(`inner', `x')wrap(inner)"),
            "(x)"
        );
    }

    // -- Edge cases --

    #[test]
    fn test_empty_define() {
        assert_eq!(run("define(`foo', `')foo."), ".");
    }

    #[test]
    fn test_define_no_args() {
        // `define` called without parens — it is recognized as a macro but
        // with no arguments, so it does nothing useful.
        assert_eq!(run("define"), "");
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(run(""), "");
    }

    #[test]
    fn test_numbers_passthrough() {
        assert_eq!(run("123 456"), "123 456");
    }

    #[test]
    fn test_special_chars() {
        assert_eq!(run("!@$%^&*"), "!@$%^&*");
    }

    #[test]
    fn test_recursive_define() {
        // A self-referencing macro recurses until the depth limit.
        // Each level appends ` bar`, and at MAX_EXPANSION_DEPTH the
        // `foo` token passes through unmatched.  The total output
        // starts with whitespace (from recursion) then `foo` and many
        // ` bar` suffixes.  We just verify it terminates and contains
        // the expected fragments.
        let out = run("define(`foo', `foo bar')foo");
        assert!(out.contains("bar"));
        assert!(out.len() > 10); // many levels of expansion.
    }

    // -- Argument parsing edge cases --

    #[test]
    fn test_args_with_nested_parens() {
        assert_eq!(run("define(`wrap', `[$1]')wrap(`(a,b)')"), "[(a,b)]");
    }

    #[test]
    fn test_args_with_quotes() {
        assert_eq!(run("define(`f', `<$1>')f(`a,b')"), "<a,b>");
    }

    // -- parse_args tests --

    #[test]
    fn test_parse_args_define() {
        let args = vec!["-Dfoo=bar".to_string(), "input.m4".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.defines.len(), 1);
        assert_eq!(opts.defines[0], ("foo".to_string(), "bar".to_string()));
        assert_eq!(opts.input_files, vec!["input.m4".to_string()]);
    }

    #[test]
    fn test_parse_args_undefine() {
        let args = vec!["-Ufoo".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.undefines, vec!["foo".to_string()]);
    }

    #[test]
    fn test_parse_args_prefix() {
        let args = vec!["-P".to_string()];
        let opts = parse_args(&args);
        assert!(opts.prefix_builtins);
    }

    // -- eval tokenizer tests --

    #[test]
    fn test_tokenize_hex() {
        let tokens = tokenize_expr("0xff").unwrap();
        assert_eq!(tokens, vec![ExprToken::Num(255)]);
    }

    #[test]
    fn test_tokenize_octal() {
        let tokens = tokenize_expr("010").unwrap();
        assert_eq!(tokens, vec![ExprToken::Num(8)]);
    }

    #[test]
    fn test_eval_div_by_zero() {
        let result = eval_expr("1 / 0");
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_mod_by_zero() {
        let result = eval_expr("1 % 0");
        assert!(result.is_err());
    }

    // -- format_radix --

    #[test]
    fn test_format_radix_binary() {
        assert_eq!(format_radix(10, 2), "1010");
    }

    #[test]
    fn test_format_radix_hex() {
        assert_eq!(format_radix(255, 16), "ff");
    }

    #[test]
    fn test_format_radix_negative() {
        assert_eq!(format_radix(-10, 10), "-10");
    }

    // -- translit expand_ranges --

    #[test]
    fn test_expand_ranges() {
        let r = expand_ranges("a-e");
        assert_eq!(r, vec!['a', 'b', 'c', 'd', 'e']);
    }

    #[test]
    fn test_expand_ranges_mixed() {
        let r = expand_ranges("a-cxyz");
        assert_eq!(r, vec!['a', 'b', 'c', 'x', 'y', 'z']);
    }

    // -- int_pow --

    #[test]
    fn test_int_pow_zero_exp() {
        assert_eq!(int_pow(5, 0), 1);
    }

    #[test]
    fn test_int_pow_negative_exp() {
        assert_eq!(int_pow(2, -3), 0);
    }

    // -- Prefix builtins mode --

    #[test]
    fn test_prefix_builtins() {
        let mut p = Processor::new_with_prefix();
        let out = p.process("m4_define(`foo', `bar')foo");
        assert_eq!(out, "bar");
    }

    #[test]
    fn test_prefix_builtins_no_bare() {
        // Without prefix, `define` should not be recognized.
        let mut p = Processor::new_with_prefix();
        let out = p.process("define(`foo', `bar')foo");
        // `define` is not a macro in -P mode, so it passes through.
        assert!(out.contains("define"));
    }

    // -- substitute_args --

    #[test]
    fn test_substitute_no_args() {
        let result = substitute_args("mac", "hello $1", &[]);
        assert_eq!(result, "hello ");
    }

    #[test]
    fn test_substitute_dollar_literal() {
        let result = substitute_args("mac", "price is $", &[]);
        assert_eq!(result, "price is $");
    }
}
