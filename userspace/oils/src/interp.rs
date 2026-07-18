//! Tree-walking interpreter for the OSH shell.
//!
//! Executes a parsed [`Program`]: variable/parameter expansion, builtins,
//! external command execution (real fork/exec via [`std::process::Command`]),
//! pipelines, redirections, command substitution, arithmetic, and control
//! flow (`if`/`while`/`until`/`for`/`case`, functions, `&&`/`||`, `;`),
//! here-documents (`<<`, `<<-`), here-strings (`<<<`), `[[ … ]]` conditional
//! expressions, and `(( … ))` arithmetic commands.
//!
//! Pathname (glob) expansion (`*.txt`, `src/*.rs`, `[abc]?.log`) applies to
//! command arguments, honoring quoting and the leading-dot rule; an unmatched
//! pattern is left literal (bash default, no `nullglob`).
//!
//! Indexed arrays are supported: `a=(x y z)`, `a[i]=v`, `a+=(w)`, `${a[i]}`,
//! `${a[@]}`/`${a[*]}`, `${#a[@]}`, and `unset a[i]`. `"${a[@]}"` preserves
//! element boundaries (one field per element).
//!
//! ## Known limitations (tracked for the grow phase — see the crate docs and
//! `design-decisions.md §72`):
//! - No *associative* arrays (`declare -A`) yet. Negative array indices and
//!   arithmetic subscripts inside `(( … ))` (`a[i]`) are not supported. Mixing
//!   a subscript with a `${a[i]:-x}`-style operator is rejected at parse time.
//! - `[[ … ]]` does not yet support `=~` (regex match — no regex engine yet);
//!   the parser rejects it with a clear message. The `-r`/`-x` file tests are
//!   approximated as "exists" pending the slateos permission model.
//! - Pipelines are *buffered*, not concurrent: each stage runs to completion
//!   and its output feeds the next. An unbounded producer (`yes | head`) will
//!   not terminate early.
//! - Redirections attach to simple commands only, not to compound commands, so
//!   `while read …; do …; done < file` is not yet supported. In particular,
//!   `read` from a here-document reads only its first line.
//! - Background (`&`) runs a single external command asynchronously; compound
//!   background jobs run synchronously.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Read, Write};
use std::process::{Command as PCommand, Stdio};

use crate::arith::{self, VarLookup};
use crate::ast::{
    AndOr, AndOrOp, ArrayIndex, AssignRhs, Assignment, CaseClause, Command, CondBinOp, CondExpr,
    ForClause, IfClause, LoopClause, ParamOp, Pipeline, Program, Redirect, RedirectOp,
    ReplaceAnchor, SimpleCommand, UnaryOp, Word, WordPart,
};
use crate::parser::parse;

/// Non-local control flow produced while executing statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Continue with the next statement.
    Next,
    /// `break N` — unwind N enclosing loops.
    Break(u32),
    /// `continue N` — restart the Nth enclosing loop.
    Continue(u32),
    /// `return` from a function/script.
    Return,
    /// `exit N` — terminate the shell.
    Exit(i32),
}

/// Where a command's standard output should go.
enum Out<'a> {
    /// Inherit the shell's real stdout.
    Inherit,
    /// Append to a capture buffer (command substitution / pipeline stage).
    Capture(&'a mut Vec<u8>),
}

/// A command's standard input source.
enum StdinSrc<'a> {
    /// Inherit the shell's real stdin.
    Inherit,
    /// Read from these bytes (previous pipeline stage / here-string).
    Bytes(&'a [u8]),
}

/// The shell interpreter and its mutable session state.
pub struct Shell {
    vars: HashMap<String, String>,
    /// Indexed arrays: `name=(a b c)` and `name[i]=v`. Kept separate from
    /// `vars`; `${name}` reads element 0, `${name[@]}`/`${name[*]}` read all.
    arrays: HashMap<String, Vec<String>>,
    exported: HashSet<String>,
    funcs: HashMap<String, Program>,
    positional: Vec<String>,
    name: String,
    last_status: i32,
    last_bg_pid: Option<u32>,
    pid: u32,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Create a fresh shell with `$0` defaulting to `osh`.
    #[must_use]
    pub fn new() -> Self {
        Shell {
            vars: HashMap::new(),
            arrays: HashMap::new(),
            exported: HashSet::new(),
            funcs: HashMap::new(),
            positional: Vec::new(),
            name: "osh".to_string(),
            last_status: 0,
            last_bg_pid: None,
            pid: std::process::id(),
        }
    }

    /// Set `$0`, the shell/script name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Set the positional parameters (`$1`, `$2`, …).
    pub fn set_positional(&mut self, args: Vec<String>) {
        self.positional = args;
    }

    /// Set a shell variable.
    pub fn set_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(name.into(), value.into());
    }

    /// The exit status of the most recently completed command.
    #[must_use]
    pub fn last_status(&self) -> i32 {
        self.last_status
    }

    /// Parse and execute shell source, returning the final exit status.
    pub fn run_source(&mut self, src: &str) -> i32 {
        let prog = match parse(src) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("osh: syntax error: {e}");
                self.last_status = 2;
                return 2;
            }
        };
        let mut out = Out::Inherit;
        match self.exec_program(&prog, &mut out) {
            Flow::Exit(code) => {
                self.last_status = code;
                code
            }
            _ => self.last_status,
        }
    }

    fn exec_program(&mut self, prog: &Program, out: &mut Out) -> Flow {
        for item in &prog.items {
            if item.background {
                // Only a single external simple command is truly backgrounded;
                // everything else runs synchronously (documented limitation).
                self.exec_background(&item.list);
                continue;
            }
            let flow = self.exec_and_or(&item.list, out, &StdinSrc::Inherit);
            match flow {
                Flow::Next => {}
                other => return other,
            }
        }
        Flow::Next
    }

    fn exec_and_or(&mut self, ao: &AndOr, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let flow = self.exec_pipeline(&ao.first, out, stdin);
        if !matches!(flow, Flow::Next) {
            return flow;
        }
        for (op, pipe) in &ao.rest {
            let run = match op {
                AndOrOp::And => self.last_status == 0,
                AndOrOp::Or => self.last_status != 0,
            };
            if run {
                let flow = self.exec_pipeline(pipe, out, stdin);
                if !matches!(flow, Flow::Next) {
                    return flow;
                }
            }
        }
        Flow::Next
    }

    fn exec_pipeline(&mut self, pipe: &Pipeline, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let flow = if pipe.commands.len() == 1 {
            self.exec_command(&pipe.commands[0], out, stdin)
        } else {
            self.exec_buffered_pipeline(&pipe.commands, out)
        };
        if pipe.negated {
            self.last_status = i32::from(self.last_status == 0);
        }
        flow
    }

    /// Run a multi-stage pipeline by buffering each stage's stdout and feeding
    /// it to the next stage's stdin. Not concurrent (see the module docs).
    fn exec_buffered_pipeline(&mut self, cmds: &[Command], out: &mut Out) -> Flow {
        let mut prev: Vec<u8> = Vec::new();
        let last = cmds.len() - 1;
        for (i, cmd) in cmds.iter().enumerate() {
            let stdin = if i == 0 {
                StdinSrc::Inherit
            } else {
                StdinSrc::Bytes(&prev)
            };
            if i == last {
                let flow = self.exec_command(cmd, out, &stdin);
                if let Flow::Exit(c) = flow {
                    return Flow::Exit(c);
                }
            } else {
                let mut buf = Vec::new();
                let mut cap = Out::Capture(&mut buf);
                let flow = self.exec_command(cmd, &mut cap, &stdin);
                if let Flow::Exit(c) = flow {
                    return Flow::Exit(c);
                }
                prev = buf;
            }
        }
        Flow::Next
    }

    fn exec_command(&mut self, cmd: &Command, out: &mut Out, stdin: &StdinSrc) -> Flow {
        match cmd {
            Command::Simple(sc) => self.exec_simple(sc, out, stdin),
            Command::If(c) => self.exec_if(c, out),
            Command::Loop(c) => self.exec_loop(c, out),
            Command::For(c) => self.exec_for(c, out),
            Command::Function(f) => {
                self.funcs.insert(f.name.clone(), f.body.clone());
                self.last_status = 0;
                Flow::Next
            }
            Command::Case(c) => self.exec_case(c, out),
            Command::Cond(e) => self.exec_cond(e),
            Command::Arith(raw) => self.exec_arith(raw),
            Command::BraceGroup(p) => self.exec_program(p, out),
            Command::Subshell(p) => {
                // A subshell gets a clone of the state; mutations don't escape.
                let mut sub = self.clone_for_subshell();
                let flow = sub.exec_program(p, out);
                self.last_status = sub.last_status;
                // Propagate an explicit exit from the subshell as a status only.
                match flow {
                    Flow::Exit(c) => {
                        self.last_status = c;
                        Flow::Next
                    }
                    _ => Flow::Next,
                }
            }
        }
    }

    fn exec_if(&mut self, c: &IfClause, out: &mut Out) -> Flow {
        let flow = self.exec_program(&c.cond, out);
        if !matches!(flow, Flow::Next) {
            return flow;
        }
        if self.last_status == 0 {
            return self.exec_program(&c.body, out);
        }
        for (cond, body) in &c.elifs {
            let flow = self.exec_program(cond, out);
            if !matches!(flow, Flow::Next) {
                return flow;
            }
            if self.last_status == 0 {
                return self.exec_program(body, out);
            }
        }
        if let Some(eb) = &c.else_body {
            return self.exec_program(eb, out);
        }
        self.last_status = 0;
        Flow::Next
    }

    fn exec_loop(&mut self, c: &LoopClause, out: &mut Out) -> Flow {
        loop {
            let flow = self.exec_program(&c.cond, out);
            if !matches!(flow, Flow::Next) {
                return flow;
            }
            let cond_true = self.last_status == 0;
            let run = if c.until { !cond_true } else { cond_true };
            if !run {
                break;
            }
            match self.exec_program(&c.body, out) {
                Flow::Next => {}
                Flow::Break(n) => {
                    if n > 1 {
                        return Flow::Break(n - 1);
                    }
                    break;
                }
                Flow::Continue(n) => {
                    if n > 1 {
                        return Flow::Continue(n - 1);
                    }
                }
                other => return other,
            }
        }
        Flow::Next
    }

    fn exec_for(&mut self, c: &ForClause, out: &mut Out) -> Flow {
        let items: Vec<String> = match &c.words {
            Some(words) => {
                let mut v = Vec::new();
                for w in words {
                    v.extend(self.expand_word(w, true));
                }
                v
            }
            None => self.positional.clone(),
        };
        for item in items {
            self.vars.insert(c.var.clone(), item);
            match self.exec_program(&c.body, out) {
                Flow::Next => {}
                Flow::Break(n) => {
                    if n > 1 {
                        return Flow::Break(n - 1);
                    }
                    break;
                }
                Flow::Continue(n) => {
                    if n > 1 {
                        return Flow::Continue(n - 1);
                    }
                }
                other => return other,
            }
        }
        Flow::Next
    }

    fn exec_case(&mut self, c: &CaseClause, out: &mut Out) -> Flow {
        let subject: Vec<char> = self.expand_to_string(&c.word).chars().collect();
        self.last_status = 0;
        for item in &c.items {
            for pat in &item.patterns {
                let pattern: Vec<char> = self.expand_to_string(pat).chars().collect();
                if glob_match(&pattern, &subject) {
                    return self.exec_program(&item.body, out);
                }
            }
        }
        Flow::Next
    }

    /// Execute a `[[ … ]]` conditional expression: exit 0 if true, 1 if false.
    fn exec_cond(&mut self, e: &CondExpr) -> Flow {
        let ok = self.cond_eval(e);
        self.last_status = i32::from(!ok);
        Flow::Next
    }

    /// Execute a `(( … ))` arithmetic command: exit 0 if the value is non-zero.
    fn exec_arith(&mut self, raw: &str) -> Flow {
        let expanded = self.expand_arith_params(raw);
        match arith::eval(&expanded, self) {
            Ok(v) => self.last_status = i32::from(v == 0),
            Err(e) => {
                eprintln!("osh: arithmetic: {e}");
                self.last_status = 1;
            }
        }
        Flow::Next
    }

    /// Evaluate a `[[ … ]]` conditional expression tree to a boolean.
    fn cond_eval(&mut self, e: &CondExpr) -> bool {
        match e {
            CondExpr::Word(w) => !self.expand_to_string(w).is_empty(),
            CondExpr::Not(inner) => !self.cond_eval(inner),
            CondExpr::And(a, b) => self.cond_eval(a) && self.cond_eval(b),
            CondExpr::Or(a, b) => self.cond_eval(a) || self.cond_eval(b),
            CondExpr::Unary(op, w) => self.cond_unary(*op, w),
            CondExpr::Binary(l, op, r) => self.cond_binary(l, *op, r),
        }
    }

    fn cond_unary(&mut self, op: UnaryOp, w: &Word) -> bool {
        // `-z`/`-n` operate on the string value; the rest are file tests.
        match op {
            UnaryOp::ZeroLen => self.expand_to_string(w).is_empty(),
            UnaryOp::NonZeroLen => !self.expand_to_string(w).is_empty(),
            _ => {
                let path = self.expand_to_string(w);
                let meta = std::fs::metadata(&path);
                match op {
                    UnaryOp::Exists => meta.is_ok(),
                    UnaryOp::File => meta.map(|m| m.is_file()).unwrap_or(false),
                    UnaryOp::Dir => meta.map(|m| m.is_dir()).unwrap_or(false),
                    UnaryOp::NonEmptyFile => {
                        meta.map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
                    }
                    // Best-effort permission tests: `-r` ≈ exists, `-w` ≈ exists
                    // and not read-only, `-x` ≈ exists. Proper mode-bit checks
                    // arrive with the slateos permission model (see todo.txt).
                    UnaryOp::Readable => meta.is_ok(),
                    UnaryOp::Writable => meta.map(|m| !m.permissions().readonly()).unwrap_or(false),
                    UnaryOp::Executable => meta.is_ok(),
                    UnaryOp::ZeroLen | UnaryOp::NonZeroLen => unreachable!(),
                }
            }
        }
    }

    fn cond_binary(&mut self, l: &Word, op: CondBinOp, r: &Word) -> bool {
        match op {
            CondBinOp::StrEq | CondBinOp::StrNe => {
                let subject: Vec<char> = self.expand_to_string(l).chars().collect();
                let rhs = self.expand_to_string(r);
                // A fully-quoted RHS is a literal; otherwise it is a glob pattern.
                let matched = if word_is_all_quoted(r) {
                    subject.iter().collect::<String>() == rhs
                } else {
                    let pat: Vec<char> = rhs.chars().collect();
                    glob_match(&pat, &subject)
                };
                if matches!(op, CondBinOp::StrEq) {
                    matched
                } else {
                    !matched
                }
            }
            CondBinOp::StrLt => self.expand_to_string(l) < self.expand_to_string(r),
            CondBinOp::StrGt => self.expand_to_string(l) > self.expand_to_string(r),
            CondBinOp::NumEq
            | CondBinOp::NumNe
            | CondBinOp::NumLt
            | CondBinOp::NumLe
            | CondBinOp::NumGt
            | CondBinOp::NumGe => {
                let a = self.eval_arith_word(l);
                let b = self.eval_arith_word(r);
                match op {
                    CondBinOp::NumEq => a == b,
                    CondBinOp::NumNe => a != b,
                    CondBinOp::NumLt => a < b,
                    CondBinOp::NumLe => a <= b,
                    CondBinOp::NumGt => a > b,
                    CondBinOp::NumGe => a >= b,
                    _ => unreachable!(),
                }
            }
        }
    }

    fn clone_for_subshell(&self) -> Shell {
        Shell {
            vars: self.vars.clone(),
            arrays: self.arrays.clone(),
            exported: self.exported.clone(),
            funcs: self.funcs.clone(),
            positional: self.positional.clone(),
            name: self.name.clone(),
            last_status: self.last_status,
            last_bg_pid: self.last_bg_pid,
            pid: self.pid,
        }
    }

    // ---- assignments and arrays ---------------------------------------------

    /// Apply a standalone assignment to shell state, handling scalars, indexed
    /// elements (`name[i]=v`), whole arrays (`name=(a b c)`), and append (`+=`).
    fn apply_assignment(&mut self, a: &Assignment) {
        match &a.value {
            AssignRhs::Scalar(w) => {
                let val = self.expand_to_string(w);
                if let Some(idx_word) = &a.index {
                    // `name[i]=val` — indexed element assignment.
                    let idx = self.eval_arith_word(idx_word);
                    let Ok(idx) = usize::try_from(idx) else {
                        eprintln!("osh: {}: bad array subscript", a.name);
                        return;
                    };
                    let arr = self.arrays.entry(a.name.clone()).or_default();
                    if idx >= arr.len() {
                        arr.resize(idx.saturating_add(1), String::new());
                    }
                    if let Some(slot) = arr.get_mut(idx) {
                        if a.append {
                            slot.push_str(&val);
                        } else {
                            *slot = val;
                        }
                    }
                } else if a.append {
                    // `name+=val` — append to the scalar (or to element 0 of an array).
                    if let Some(arr) = self.arrays.get_mut(&a.name) {
                        if let Some(first) = arr.first_mut() {
                            first.push_str(&val);
                        } else {
                            arr.push(val);
                        }
                    } else {
                        let cur = self.vars.get(&a.name).cloned().unwrap_or_default();
                        self.vars.insert(a.name.clone(), cur + &val);
                    }
                } else {
                    self.vars.insert(a.name.clone(), val);
                }
            }
            AssignRhs::Array(words) => {
                let mut elems: Vec<String> = Vec::new();
                for w in words {
                    elems.extend(self.expand_word(w, true));
                }
                if a.append {
                    self.arrays.entry(a.name.clone()).or_default().extend(elems);
                } else {
                    self.arrays.insert(a.name.clone(), elems);
                }
            }
        }
    }

    /// Collapse an assignment into a `(name, value)` pair for command-prefix use
    /// (`FOO=bar cmd`). Arrays join their elements with a single space.
    fn assignment_prefix_value(&mut self, a: &Assignment) -> (String, String) {
        let val = match &a.value {
            AssignRhs::Scalar(w) => self.expand_to_string(w),
            AssignRhs::Array(words) => {
                let mut elems: Vec<String> = Vec::new();
                for w in words {
                    elems.extend(self.expand_word(w, true));
                }
                elems.join(" ")
            }
        };
        (a.name.clone(), val)
    }

    /// All elements of `name`, treating a plain scalar as a one-element array.
    fn array_elements(&self, name: &str) -> Vec<String> {
        if let Some(a) = self.arrays.get(name) {
            a.clone()
        } else if let Some(v) = self.vars.get(name) {
            vec![v.clone()]
        } else {
            Vec::new()
        }
    }

    /// A single array element by index (scalar acts as element 0). `None` if the
    /// index is out of range or negative.
    fn array_element(&self, name: &str, idx: i64) -> Option<String> {
        let idx = usize::try_from(idx).ok()?;
        if let Some(a) = self.arrays.get(name) {
            a.get(idx).cloned()
        } else if idx == 0 {
            self.vars.get(name).cloned()
        } else {
            None
        }
    }

    /// Expand `${name[index]}` / `${name[@]}` / `${#name[@]}` to a string
    /// (scalar context; `[@]`/`[*]` join with a space).
    fn expand_array_ref(&mut self, name: &str, index: &ArrayIndex, length: bool) -> String {
        match index {
            ArrayIndex::All | ArrayIndex::Star => {
                let elems = self.array_elements(name);
                if length {
                    elems.len().to_string()
                } else {
                    elems.join(" ")
                }
            }
            ArrayIndex::Index(w) => {
                let idx = self.eval_arith_word(w);
                let val = self.array_element(name, idx);
                if length {
                    val.map_or(0, |v| v.chars().count()).to_string()
                } else {
                    val.unwrap_or_default()
                }
            }
        }
    }

    // ---- simple command execution -------------------------------------------

    fn exec_simple(&mut self, sc: &SimpleCommand, out: &mut Out, stdin: &StdinSrc) -> Flow {
        // Expand the command words into argv (with the current variable values,
        // before any prefix assignments take effect).
        let mut argv: Vec<String> = Vec::new();
        for w in &sc.words {
            argv.extend(self.expand_word(w, true));
        }

        // Pure assignment (no command word): persist the variables/arrays.
        if argv.is_empty() {
            for a in &sc.assignments {
                self.apply_assignment(a);
            }
            self.last_status = 0;
            return Flow::Next;
        }

        // Command present: build scalar env prefixes (`FOO=bar cmd`). Array and
        // indexed prefix assignments collapse to a space-joined scalar.
        let mut assigns: Vec<(String, String)> = Vec::with_capacity(sc.assignments.len());
        for a in &sc.assignments {
            assigns.push(self.assignment_prefix_value(a));
        }

        // Resolve redirections (targets are expanded now).
        let redir = match self.resolve_redirects(&sc.redirects) {
            Ok(r) => r,
            Err(msg) => {
                eprintln!("osh: {msg}");
                self.last_status = 1;
                return Flow::Next;
            }
        };

        let name = argv[0].clone();

        // Function?
        if self.funcs.contains_key(&name) {
            return self.call_function(&name, &argv[1..], &assigns, out, stdin, &redir);
        }

        // Builtin?
        if is_builtin(&name) {
            return self.run_builtin(&name, &argv, &assigns, out, stdin, &redir);
        }

        // External command.
        self.run_external(&argv, &assigns, out, stdin, &redir);
        Flow::Next
    }

    fn call_function(
        &mut self,
        name: &str,
        args: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        _stdin: &StdinSrc,
        _redir: &RedirPlan,
    ) -> Flow {
        let Some(body) = self.funcs.get(name).cloned() else {
            self.last_status = 127;
            return Flow::Next;
        };
        // Temporarily apply assignments and swap positionals.
        let saved_pos = std::mem::replace(&mut self.positional, args.to_vec());
        let saved: Vec<(String, Option<String>)> = assigns
            .iter()
            .map(|(k, v)| {
                let old = self.vars.insert(k.clone(), v.clone());
                (k.clone(), old)
            })
            .collect();

        let flow = self.exec_program(&body, out);

        self.positional = saved_pos;
        for (k, old) in saved {
            match old {
                Some(v) => {
                    self.vars.insert(k, v);
                }
                None => {
                    self.vars.remove(&k);
                }
            }
        }
        match flow {
            Flow::Return | Flow::Next => Flow::Next,
            other => other,
        }
    }

    fn run_external(
        &mut self,
        argv: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        stdin: &StdinSrc,
        redir: &RedirPlan,
    ) {
        let mut cmd = PCommand::new(&argv[0]);
        cmd.args(&argv[1..]);

        // Environment: exported shell vars + this command's temp assignments.
        for (k, v) in &self.vars {
            if self.exported.contains(k) {
                cmd.env(k, v);
            }
        }
        for (k, v) in assigns {
            cmd.env(k, v);
        }

        // stdin — a here-doc/here-string body takes precedence, then a file
        // redirect, then the inherited pipeline input.
        let mut input_bytes: Option<Vec<u8>> = None;
        if let Some(data) = &redir.stdin_data {
            input_bytes = Some(data.clone());
            cmd.stdin(Stdio::piped());
        } else {
            match &redir.stdin {
                Some(path) => match std::fs::File::open(path) {
                    Ok(f) => {
                        cmd.stdin(Stdio::from(f));
                    }
                    Err(e) => {
                        eprintln!("osh: {path}: {e}");
                        self.last_status = 1;
                        return;
                    }
                },
                None => match stdin {
                    StdinSrc::Inherit => {
                        cmd.stdin(Stdio::inherit());
                    }
                    StdinSrc::Bytes(b) => {
                        input_bytes = Some(b.to_vec());
                        cmd.stdin(Stdio::piped());
                    }
                },
            }
        }

        // stdout
        let capturing = matches!(out, Out::Capture(_)) && redir.stdout.is_none();
        match &redir.stdout {
            Some((path, append)) => match open_out(path, *append) {
                Ok(f) => {
                    cmd.stdout(Stdio::from(f));
                }
                Err(e) => {
                    eprintln!("osh: {path}: {e}");
                    self.last_status = 1;
                    return;
                }
            },
            None => {
                if capturing {
                    cmd.stdout(Stdio::piped());
                } else {
                    cmd.stdout(Stdio::inherit());
                }
            }
        }

        // stderr
        if let Some((path, append)) = &redir.stderr {
            match open_out(path, *append) {
                Ok(f) => {
                    cmd.stderr(Stdio::from(f));
                }
                Err(e) => {
                    eprintln!("osh: {path}: {e}");
                    self.last_status = 1;
                    return;
                }
            }
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    eprintln!("osh: {}: command not found", argv[0]);
                    self.last_status = 127;
                } else {
                    eprintln!("osh: {}: {e}", argv[0]);
                    self.last_status = 126;
                }
                return;
            }
        };

        if let Some(bytes) = input_bytes
            && let Some(mut si) = child.stdin.take()
        {
            let _ = si.write_all(&bytes); // child may exit early; ignore EPIPE
        }

        if capturing {
            let mut captured = Vec::new();
            if let Some(mut so) = child.stdout.take() {
                let _ = so.read_to_end(&mut captured);
            }
            if let Out::Capture(buf) = out {
                buf.extend_from_slice(&captured);
            }
        }

        match child.wait() {
            Ok(status) => {
                self.last_status = status.code().unwrap_or(1);
            }
            Err(e) => {
                eprintln!("osh: wait failed: {e}");
                self.last_status = 1;
            }
        }
    }

    fn exec_background(&mut self, ao: &AndOr) {
        // Only handle the common case: a single external simple command.
        if ao.rest.is_empty()
            && ao.first.commands.len() == 1
            && !ao.first.negated
            && let Command::Simple(sc) = &ao.first.commands[0]
        {
            let mut argv = Vec::new();
            for w in &sc.words {
                argv.extend(self.expand_word(w, true));
            }
            if !argv.is_empty() && !self.funcs.contains_key(&argv[0]) && !is_builtin(&argv[0]) {
                let mut cmd = PCommand::new(&argv[0]);
                cmd.args(&argv[1..]);
                for (k, v) in &self.vars {
                    if self.exported.contains(k) {
                        cmd.env(k, v);
                    }
                }
                match cmd.spawn() {
                    Ok(child) => {
                        self.last_bg_pid = Some(child.id());
                        self.last_status = 0;
                        return;
                    }
                    Err(e) => {
                        eprintln!("osh: {}: {e}", argv[0]);
                        self.last_status = 1;
                        return;
                    }
                }
            }
        }
        // Fallback: run synchronously.
        let mut out = Out::Inherit;
        let _ = self.exec_and_or(ao, &mut out, &StdinSrc::Inherit);
    }

    // ---- redirection resolution ---------------------------------------------

    fn resolve_redirects(&mut self, redirs: &[Redirect]) -> Result<RedirPlan, String> {
        let mut plan = RedirPlan::default();
        for r in redirs {
            match r.op {
                RedirectOp::Read => {
                    if r.fd == 0 {
                        plan.stdin = Some(self.expand_to_string(&r.target));
                        plan.stdin_data = None;
                    }
                }
                RedirectOp::HereDoc => {
                    if r.fd == 0 {
                        // Here-doc bodies expand like a double-quoted context:
                        // no tilde expansion, no field splitting, no globbing.
                        let body = self.expand_double_quoted(&r.target.parts);
                        plan.stdin = None;
                        plan.stdin_data = Some(body.into_bytes());
                    }
                }
                RedirectOp::HereStr => {
                    if r.fd == 0 {
                        let mut s = self.expand_to_string(&r.target);
                        s.push('\n');
                        plan.stdin = None;
                        plan.stdin_data = Some(s.into_bytes());
                    }
                }
                RedirectOp::Write | RedirectOp::Append => {
                    let target = self.expand_to_string(&r.target);
                    let append = matches!(r.op, RedirectOp::Append);
                    match r.fd {
                        2 => plan.stderr = Some((target, append)),
                        _ => plan.stdout = Some((target, append)),
                    }
                }
                RedirectOp::DupOut => {
                    // `2>&1` → stderr follows stdout; `1>&2` → the reverse.
                    let target = self.expand_to_string(&r.target);
                    if r.fd == 2 && target == "1" {
                        plan.stderr = plan.stdout.clone();
                    } else if r.fd == 1 && target == "2" {
                        plan.stdout = plan.stderr.clone();
                    }
                }
            }
        }
        Ok(plan)
    }

    // ---- expansion ----------------------------------------------------------

    /// Expand a word, optionally field-splitting the results of unquoted
    /// expansions. Returns zero or more fields.
    fn expand_word(&mut self, word: &Word, split: bool) -> Vec<String> {
        if split {
            // Command-argument context: field-split unquoted expansions, then
            // apply pathname (glob) expansion to each resulting field.
            let fields = self.expand_word_annotated(word);
            let mut out = Vec::new();
            for f in fields {
                glob_or_literal(&f, &mut out);
            }
            return out;
        }
        // Non-splitting context (assignment values, redirect targets, `[[ ]]`
        // operands): concatenate everything into one field, no splitting/glob.
        let mut cur = String::new();
        let mut started = false;
        for (idx, part) in word.parts.iter().enumerate() {
            match part {
                WordPart::Literal(s) => {
                    let s = if idx == 0 {
                        self.tilde_expand(s)
                    } else {
                        s.clone()
                    };
                    cur.push_str(&s);
                    started = true;
                }
                WordPart::SingleQuoted(s) => {
                    cur.push_str(s);
                    started = true;
                }
                WordPart::DoubleQuoted(parts) => {
                    cur.push_str(&self.expand_double_quoted(parts));
                    started = true;
                }
                other => {
                    cur.push_str(&self.expand_dynamic(other));
                    started = true;
                }
            }
        }
        if started {
            vec![cur]
        } else {
            Vec::new()
        }
    }

    /// Expand a word into fields of quote-annotated characters (splitting
    /// unquoted expansions on IFS). The quoting flag lets a later glob step
    /// treat quoted metacharacters as literals.
    fn expand_word_annotated(&mut self, word: &Word) -> Vec<Vec<EChar>> {
        let mut fields: Vec<Vec<EChar>> = Vec::new();
        let mut cur: Vec<EChar> = Vec::new();
        let mut started = false;
        for (idx, part) in word.parts.iter().enumerate() {
            match part {
                WordPart::Literal(s) => {
                    let s = if idx == 0 {
                        self.tilde_expand(s)
                    } else {
                        s.clone()
                    };
                    push_chars(&mut cur, &s, false);
                    started = true;
                }
                WordPart::SingleQuoted(s) => {
                    push_chars(&mut cur, s, true);
                    started = true;
                }
                WordPart::DoubleQuoted(parts) => {
                    // `"${arr[@]}"` (and `"$@"`) expand to one field per element,
                    // preserving embedded whitespace; empty arrays yield no field.
                    if let [
                        WordPart::ArrayRef {
                            name,
                            index: ArrayIndex::All,
                            length: false,
                        },
                    ] = parts.as_slice()
                    {
                        for (i, el) in self.array_elements(name).into_iter().enumerate() {
                            if i > 0 {
                                fields.push(std::mem::take(&mut cur));
                            }
                            push_chars(&mut cur, &el, true);
                            started = true;
                        }
                    } else {
                        let s = self.expand_double_quoted(parts);
                        push_chars(&mut cur, &s, true);
                        started = true;
                    }
                }
                other => {
                    let val = self.expand_dynamic(other);
                    let pieces = split_ifs(&val);
                    if !pieces.is_empty() {
                        push_chars(&mut cur, &pieces[0], false);
                        started = true;
                        for extra in &pieces[1..] {
                            fields.push(std::mem::take(&mut cur));
                            push_chars(&mut cur, extra, false);
                        }
                    }
                }
            }
        }
        if started {
            fields.push(cur);
        }
        fields
    }

    /// Expand a word to a single string (no field splitting) — used for
    /// assignment values and redirection targets.
    fn expand_to_string(&mut self, word: &Word) -> String {
        let fields = self.expand_word(word, false);
        fields.join("")
    }

    fn expand_double_quoted(&mut self, parts: &[WordPart]) -> String {
        let mut s = String::new();
        for part in parts {
            match part {
                WordPart::Literal(t) | WordPart::SingleQuoted(t) => s.push_str(t),
                other => s.push_str(&self.expand_dynamic(other)),
            }
        }
        s
    }

    /// Expand a dynamic word part (parameter/command/arithmetic) to a string.
    fn expand_dynamic(&mut self, part: &WordPart) -> String {
        match part {
            WordPart::Param(name) => self.param_value(name).unwrap_or_default(),
            WordPart::Length(name) => self
                .param_value(name)
                .map_or(0, |v| v.chars().count())
                .to_string(),
            WordPart::ParamOp { name, op, arg } => self.expand_param_op(name, *op, arg),
            WordPart::ParamTrim {
                name,
                suffix,
                longest,
                pattern,
            } => {
                let value = self.param_value(name).unwrap_or_default();
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                param_trim(&value, &pat, *suffix, *longest)
            }
            WordPart::ParamSubstr {
                name,
                offset,
                length,
            } => {
                let value = self.param_value(name).unwrap_or_default();
                let off = self.eval_arith_word(offset);
                let len = length.as_ref().map(|l| self.eval_arith_word(l));
                param_substr(&value, off, len)
            }
            WordPart::ParamReplace {
                name,
                all,
                anchor,
                pattern,
                replacement,
            } => {
                let value = self.param_value(name).unwrap_or_default();
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                let repl = self.expand_to_string(replacement);
                param_replace(&value, &pat, &repl, *all, *anchor)
            }
            WordPart::CommandSub(prog) => self.command_sub(prog),
            WordPart::ArithSub(expr) => self.arith_sub(expr),
            WordPart::ArrayRef {
                name,
                index,
                length,
            } => self.expand_array_ref(name, index, *length),
            // Literal/quoted handled by callers.
            WordPart::Literal(s) | WordPart::SingleQuoted(s) => s.clone(),
            WordPart::DoubleQuoted(parts) => self.expand_double_quoted(parts),
        }
    }

    fn expand_param_op(&mut self, name: &str, op: ParamOp, arg: &Word) -> String {
        let cur = self.param_value(name);
        let is_set_nonempty = cur.as_ref().is_some_and(|v| !v.is_empty());
        match op {
            ParamOp::UseDefault => {
                if is_set_nonempty {
                    cur.unwrap_or_default()
                } else {
                    self.expand_to_string(arg)
                }
            }
            ParamOp::AssignDefault => {
                if is_set_nonempty {
                    cur.unwrap_or_default()
                } else {
                    let v = self.expand_to_string(arg);
                    self.vars.insert(name.to_string(), v.clone());
                    v
                }
            }
            ParamOp::UseAlternate => {
                if is_set_nonempty {
                    self.expand_to_string(arg)
                } else {
                    String::new()
                }
            }
            ParamOp::ErrorIfUnset => {
                if is_set_nonempty {
                    cur.unwrap_or_default()
                } else {
                    let msg = self.expand_to_string(arg);
                    eprintln!(
                        "osh: {name}: {}",
                        if msg.is_empty() {
                            "parameter null or not set"
                        } else {
                            &msg
                        }
                    );
                    String::new()
                }
            }
        }
    }

    /// Resolve a parameter's value; `None` means unset.
    fn param_value(&self, name: &str) -> Option<String> {
        match name {
            "?" => Some(self.last_status.to_string()),
            "#" => Some(self.positional.len().to_string()),
            "$" => Some(self.pid.to_string()),
            "!" => self.last_bg_pid.map(|p| p.to_string()),
            "@" | "*" => Some(self.positional.join(" ")),
            "0" => Some(self.name.clone()),
            "-" => Some(String::new()),
            _ => {
                if let Ok(n) = name.parse::<usize>() {
                    if n == 0 {
                        return Some(self.name.clone());
                    }
                    return self.positional.get(n - 1).cloned();
                }
                // A plain array reference (`$arr` / `${arr}`) reads element 0.
                if let Some(arr) = self.arrays.get(name) {
                    return arr.first().cloned();
                }
                self.vars
                    .get(name)
                    .cloned()
                    .or_else(|| std::env::var(name).ok())
            }
        }
    }

    fn command_sub(&mut self, prog: &Program) -> String {
        let mut buf = Vec::new();
        {
            let mut out = Out::Capture(&mut buf);
            let _ = self.exec_program(prog, &mut out);
        }
        let mut s = String::from_utf8_lossy(&buf).into_owned();
        // Strip trailing newlines, as command substitution does.
        while s.ends_with('\n') {
            s.pop();
        }
        s
    }

    /// Expand a word to a string and evaluate it as an arithmetic expression
    /// (used for `${name:offset:length}`). Errors/empties yield `0`.
    fn eval_arith_word(&mut self, w: &Word) -> i64 {
        let s = self.expand_to_string(w);
        let s = s.trim();
        if s.is_empty() {
            return 0;
        }
        arith::eval(s, self).unwrap_or(0)
    }

    fn arith_sub(&mut self, expr: &str) -> String {
        // Expand `$name` / `${name}` parameters inside the expression first;
        // bare identifiers are resolved by the evaluator via `VarLookup`.
        let expanded = self.expand_arith_params(expr);
        match arith::eval(&expanded, self) {
            Ok(v) => v.to_string(),
            Err(e) => {
                eprintln!("osh: arithmetic: {e}");
                "0".to_string()
            }
        }
    }

    /// Replace `$name`, `${name}`, and `$1` inside an arithmetic string with
    /// the parameter's (numeric) value.
    fn expand_arith_params(&self, expr: &str) -> String {
        let chars: Vec<char> = expr.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' {
                i += 1;
                let name = if chars.get(i) == Some(&'{') {
                    i += 1;
                    let mut n = String::new();
                    while i < chars.len() && chars[i] != '}' {
                        n.push(chars[i]);
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1; // consume '}'
                    }
                    n
                } else {
                    let mut n = String::new();
                    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                        n.push(chars[i]);
                        i += 1;
                    }
                    n
                };
                let val = self.param_value(&name).unwrap_or_default();
                let val = val.trim();
                out.push_str(if val.is_empty() { "0" } else { val });
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    fn tilde_expand(&self, s: &str) -> String {
        if s == "~" {
            return self.param_value("HOME").unwrap_or_else(|| "~".to_string());
        }
        if let Some(rest) = s.strip_prefix("~/")
            && let Some(home) = self.param_value("HOME")
        {
            return format!("{home}/{rest}");
        }
        s.to_string()
    }

    // ---- builtins -----------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn run_builtin(
        &mut self,
        name: &str,
        argv: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        stdin: &StdinSrc,
        redir: &RedirPlan,
    ) -> Flow {
        // Apply temporary assignments for the duration of the builtin.
        let saved: Vec<(String, Option<String>)> = assigns
            .iter()
            .map(|(k, v)| (k.clone(), self.vars.insert(k.clone(), v.clone())))
            .collect();

        let mut flow = Flow::Next;
        let args = &argv[1..];
        let status = match name {
            ":" | "true" => 0,
            "false" => 1,
            "cd" => self.builtin_cd(args),
            "pwd" => self.builtin_pwd(out, redir),
            "echo" => self.builtin_echo(args, out, redir),
            "printf" => self.builtin_printf(args, out, redir),
            "export" => self.builtin_export(args),
            "unset" => self.builtin_unset(args),
            "set" => self.builtin_set(args),
            "shift" => self.builtin_shift(args),
            "read" => self.builtin_read(args, stdin, redir),
            "test" | "[" => self.builtin_test(name, args),
            "eval" => {
                let joined = args.join(" ");
                self.run_source(&joined)
            }
            "source" | "." => self.builtin_source(args),
            "type" => self.builtin_type(args, out, redir),
            "exit" => {
                let code = args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(self.last_status);
                flow = Flow::Exit(code);
                code
            }
            "return" => {
                let code = args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(self.last_status);
                flow = Flow::Return;
                code
            }
            "break" => {
                let n = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                flow = Flow::Break(n.max(1));
                0
            }
            "continue" => {
                let n = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                flow = Flow::Continue(n.max(1));
                0
            }
            _ => {
                eprintln!("osh: {name}: not a builtin");
                127
            }
        };

        // Restore temporary assignments (builtins don't persist them, except
        // pure-assignment which never reaches here).
        for (k, old) in saved {
            match old {
                Some(v) => {
                    self.vars.insert(k, v);
                }
                None => {
                    self.vars.remove(&k);
                }
            }
        }

        self.last_status = status;
        flow
    }

    fn builtin_cd(&mut self, args: &[String]) -> i32 {
        let target = match args.first() {
            Some(p) => p.clone(),
            None => self.param_value("HOME").unwrap_or_else(|| "/".to_string()),
        };
        match std::env::set_current_dir(&target) {
            Ok(()) => {
                if let Ok(cwd) = std::env::current_dir() {
                    self.vars
                        .insert("PWD".to_string(), cwd.to_string_lossy().into_owned());
                }
                0
            }
            Err(e) => {
                eprintln!("osh: cd: {target}: {e}");
                1
            }
        }
    }

    fn builtin_pwd(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.write_line(out, redir, &cwd)
    }

    fn builtin_echo(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut newline = true;
        let mut start = 0;
        if args.first().map(String::as_str) == Some("-n") {
            newline = false;
            start = 1;
        }
        let mut line = args[start..].join(" ");
        if newline {
            line.push('\n');
        }
        self.write_bytes(out, redir, line.as_bytes())
    }

    fn builtin_printf(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let Some(fmt) = args.first() else {
            return 0;
        };
        let text = format_printf(fmt, &args[1..]);
        self.write_bytes(out, redir, text.as_bytes())
    }

    fn builtin_export(&mut self, args: &[String]) -> i32 {
        for a in args {
            if let Some(eq) = a.find('=') {
                let (k, v) = (a[..eq].to_string(), a[eq + 1..].to_string());
                self.vars.insert(k.clone(), v);
                self.exported.insert(k);
            } else {
                self.exported.insert(a.clone());
            }
        }
        0
    }

    fn builtin_unset(&mut self, args: &[String]) -> i32 {
        for a in args {
            // `unset name[i]` removes a single element; `unset name` removes the
            // whole variable/array/function.
            if let Some(open) = a.find('[')
                && a.ends_with(']')
            {
                let name = &a[..open];
                let idx_src = &a[open + 1..a.len() - 1];
                if let Some(arr) = self.arrays.get_mut(name)
                    && let Ok(idx) = idx_src.parse::<usize>()
                    && idx < arr.len()
                {
                    arr.remove(idx);
                }
                continue;
            }
            self.vars.remove(a);
            self.arrays.remove(a);
            self.exported.remove(a);
            self.funcs.remove(a);
        }
        0
    }

    fn builtin_set(&mut self, args: &[String]) -> i32 {
        // `set -- a b c` replaces the positional parameters.
        if args.first().map(String::as_str) == Some("--") {
            self.positional = args[1..].to_vec();
        } else if !args.is_empty() && !args[0].starts_with('-') {
            self.positional = args.to_vec();
        }
        0
    }

    fn builtin_shift(&mut self, args: &[String]) -> i32 {
        let n = args.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
        if n <= self.positional.len() {
            self.positional.drain(..n);
            0
        } else {
            1
        }
    }

    fn builtin_read(&mut self, args: &[String], stdin: &StdinSrc, redir: &RedirPlan) -> i32 {
        let line = match self.read_line(stdin, redir) {
            Some(l) => l,
            None => return 1, // EOF
        };
        let names: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
        if names.is_empty() {
            self.vars.insert("REPLY".to_string(), line);
            return 0;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        for (i, name) in names.iter().enumerate() {
            let val = if i + 1 == names.len() {
                // Last variable gets the remaining fields joined.
                fields[i.min(fields.len())..].join(" ")
            } else {
                fields.get(i).map_or(String::new(), |s| (*s).to_string())
            };
            self.vars.insert((*name).clone(), val);
        }
        0
    }

    fn builtin_source(&mut self, args: &[String]) -> i32 {
        let Some(path) = args.first() else {
            eprintln!("osh: source: filename argument required");
            return 2;
        };
        match std::fs::read_to_string(path) {
            Ok(src) => {
                let saved = if args.len() > 1 {
                    Some(std::mem::replace(&mut self.positional, args[1..].to_vec()))
                } else {
                    None
                };
                let code = self.run_source(&src);
                if let Some(p) = saved {
                    self.positional = p;
                }
                code
            }
            Err(e) => {
                eprintln!("osh: source: {path}: {e}");
                1
            }
        }
    }

    fn builtin_type(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut status = 0;
        for a in args {
            let desc = if self.funcs.contains_key(a) {
                format!("{a} is a function")
            } else if is_builtin(a) {
                format!("{a} is a shell builtin")
            } else {
                status = 1;
                format!("{a}: not found")
            };
            let _ = self.write_line(out, redir, &desc);
        }
        status
    }

    fn builtin_test(&mut self, name: &str, args: &[String]) -> i32 {
        // For `[`, the last argument must be `]`.
        let mut a: Vec<&str> = args.iter().map(String::as_str).collect();
        if name == "[" {
            if a.last() == Some(&"]") {
                a.pop();
            } else {
                eprintln!("osh: [: missing ']'");
                return 2;
            }
        }
        i32::from(!eval_test(&a))
    }

    // ---- output helpers -----------------------------------------------------

    fn write_line(&mut self, out: &mut Out, redir: &RedirPlan, line: &str) -> i32 {
        let mut s = line.to_string();
        s.push('\n');
        self.write_bytes(out, redir, s.as_bytes())
    }

    fn write_bytes(&mut self, out: &mut Out, redir: &RedirPlan, bytes: &[u8]) -> i32 {
        // A `>`/`>>` redirect on the builtin wins over the ambient sink.
        if let Some((path, append)) = &redir.stdout {
            match open_out(path, *append) {
                Ok(mut f) => {
                    if f.write_all(bytes).is_err() {
                        return 1;
                    }
                    0
                }
                Err(e) => {
                    eprintln!("osh: {path}: {e}");
                    1
                }
            }
        } else {
            match out {
                Out::Capture(buf) => {
                    buf.extend_from_slice(bytes);
                    0
                }
                Out::Inherit => {
                    let stdout = io::stdout();
                    let mut lock = stdout.lock();
                    if lock.write_all(bytes).is_err() {
                        return 1;
                    }
                    let _ = lock.flush();
                    0
                }
            }
        }
    }

    fn read_line(&self, stdin: &StdinSrc, redir: &RedirPlan) -> Option<String> {
        if let Some(data) = &redir.stdin_data {
            // Here-doc/here-string: read the first line. (Multi-line `read`
            // loops over here-docs require compound-command redirects, which are
            // not yet supported — see the module limitations.)
            let mut r = io::BufReader::new(&data[..]);
            return read_one_line(&mut r);
        }
        if let Some(path) = &redir.stdin {
            let f = std::fs::File::open(path).ok()?;
            let mut r = io::BufReader::new(f);
            return read_one_line(&mut r);
        }
        match stdin {
            StdinSrc::Bytes(b) => {
                let mut r = io::BufReader::new(*b);
                read_one_line(&mut r)
            }
            StdinSrc::Inherit => {
                let stdin = io::stdin();
                let mut lock = stdin.lock();
                read_one_line(&mut lock)
            }
        }
    }
}

/// Let the arithmetic evaluator read shell variables.
impl VarLookup for Shell {
    fn get(&self, name: &str) -> Option<i64> {
        self.param_value(name).and_then(|v| v.trim().parse::<i64>().ok())
    }
}

// ---- free helpers -----------------------------------------------------------

/// Per-command redirection plan (expanded targets).
#[derive(Debug, Clone, Default)]
struct RedirPlan {
    stdin: Option<String>,
    /// In-memory stdin bytes from a here-document / here-string (takes
    /// precedence over `stdin` and the inherited pipeline input).
    stdin_data: Option<Vec<u8>>,
    stdout: Option<(String, bool)>,
    stderr: Option<(String, bool)>,
}

/// A single expanded character tagged with whether it came from a quoted
/// context. Quoted characters are exempt from field splitting (already done)
/// and pathname (glob) expansion — a quoted `*` matches a literal `*`.
#[derive(Clone, Copy)]
struct EChar {
    c: char,
    quoted: bool,
}

/// Append the characters of `s` to `buf`, tagging each with `quoted`.
fn push_chars(buf: &mut Vec<EChar>, s: &str, quoted: bool) {
    for c in s.chars() {
        buf.push(EChar { c, quoted });
    }
}

/// Apply pathname expansion to one annotated field, pushing the results (or the
/// literal field, if it has no unquoted metacharacter or matches nothing) into
/// `out`. This implements bash's default (no-`nullglob`) behavior: an
/// unmatched pattern is left as the literal word.
fn glob_or_literal(field: &[EChar], out: &mut Vec<String>) {
    let has_meta = field
        .iter()
        .any(|e| !e.quoted && matches!(e.c, '*' | '?' | '['));
    let literal: String = field.iter().map(|e| e.c).collect();
    if !has_meta {
        out.push(literal);
        return;
    }
    let mut matches = glob_expand_field(field);
    if matches.is_empty() {
        out.push(literal);
    } else {
        matches.sort();
        out.append(&mut matches);
    }
}

/// A compiled glob pattern token (for one path component).
enum PatTok {
    /// `*` — match any run of characters.
    Star,
    /// `?` — match any single character.
    Any,
    /// A literal character (either an ordinary char or a quoted metacharacter).
    Lit(char),
    /// `[...]` character class.
    Class { negate: bool, items: Vec<ClassItem> },
}

enum ClassItem {
    Ch(char),
    Range(char, char),
}

/// Compile one annotated path component into glob tokens. Quoted characters are
/// always literal; unquoted `* ? [` are special.
fn compile_glob(comp: &[EChar]) -> Vec<PatTok> {
    let mut toks = Vec::new();
    let mut i = 0;
    while i < comp.len() {
        let e = comp[i];
        if e.quoted {
            toks.push(PatTok::Lit(e.c));
            i += 1;
            continue;
        }
        match e.c {
            '*' => {
                toks.push(PatTok::Star);
                i += 1;
            }
            '?' => {
                toks.push(PatTok::Any);
                i += 1;
            }
            '[' => {
                if let Some((tok, next)) = compile_class(comp, i) {
                    toks.push(tok);
                    i = next;
                } else {
                    toks.push(PatTok::Lit('['));
                    i += 1;
                }
            }
            c => {
                toks.push(PatTok::Lit(c));
                i += 1;
            }
        }
    }
    toks
}

/// Compile a `[...]` class starting at `comp[start] == '['`. Returns the token
/// and the index just past the closing `]`, or `None` if unterminated.
fn compile_class(comp: &[EChar], start: usize) -> Option<(PatTok, usize)> {
    let mut i = start + 1;
    let mut negate = false;
    if matches!(comp.get(i).map(|e| e.c), Some('!' | '^')) {
        negate = true;
        i += 1;
    }
    let mut items = Vec::new();
    let mut first = true;
    while i < comp.len() {
        let c = comp[i].c;
        if c == ']' && !first {
            return Some((PatTok::Class { negate, items }, i + 1));
        }
        first = false;
        if i + 2 < comp.len() && comp[i + 1].c == '-' && comp[i + 2].c != ']' {
            items.push(ClassItem::Range(c, comp[i + 2].c));
            i += 3;
        } else {
            items.push(ClassItem::Ch(c));
            i += 1;
        }
    }
    None
}

/// Match a compiled glob against a filename (anchored, star-backtracking).
fn match_glob_toks(toks: &[PatTok], name: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < name.len() {
        if matches!(toks.get(pi), Some(PatTok::Star)) {
            star = Some((pi, ti));
            pi += 1;
            continue;
        }
        let matched = match toks.get(pi) {
            Some(PatTok::Any) => true,
            Some(PatTok::Lit(c)) => *c == name[ti],
            Some(PatTok::Class { negate, items }) => {
                class_matches(items, name[ti]) ^ *negate
            }
            _ => false,
        };
        if matched {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while matches!(toks.get(pi), Some(PatTok::Star)) {
        pi += 1;
    }
    pi == toks.len()
}

fn class_matches(items: &[ClassItem], ch: char) -> bool {
    items.iter().any(|it| match it {
        ClassItem::Ch(c) => *c == ch,
        ClassItem::Range(a, b) => *a <= ch && ch <= *b,
    })
}

/// Whether a compiled component's first token is a literal `.` — controls the
/// hidden-file rule (a leading `.` in a name is only matched explicitly).
fn glob_starts_with_dot(toks: &[PatTok]) -> bool {
    matches!(toks.first(), Some(PatTok::Lit('.')))
}

/// Expand an annotated field containing at least one unquoted metacharacter
/// against the filesystem, returning the matching paths (unsorted).
fn glob_expand_field(field: &[EChar]) -> Vec<String> {
    let absolute = field.first().is_some_and(|e| e.c == '/');
    // Split into non-empty components on '/'.
    let mut comps: Vec<Vec<EChar>> = Vec::new();
    let mut cur: Vec<EChar> = Vec::new();
    for &e in field {
        if e.c == '/' {
            if !cur.is_empty() {
                comps.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(e);
        }
    }
    if !cur.is_empty() {
        comps.push(cur);
    }
    if comps.is_empty() {
        return Vec::new();
    }
    let mut cands: Vec<String> = vec![if absolute { "/".to_string() } else { String::new() }];
    for comp in &comps {
        let has_meta = comp
            .iter()
            .any(|e| !e.quoted && matches!(e.c, '*' | '?' | '['));
        let comp_literal: String = comp.iter().map(|e| e.c).collect();
        let mut next: Vec<String> = Vec::new();
        for base in &cands {
            if has_meta {
                let dir = if base.is_empty() { "." } else { base.as_str() };
                let toks = compile_glob(comp);
                let allow_dot = glob_starts_with_dot(&toks);
                let Ok(rd) = std::fs::read_dir(dir) else {
                    continue;
                };
                let mut names: Vec<String> = Vec::new();
                for ent in rd.flatten() {
                    let name = ent.file_name().to_string_lossy().into_owned();
                    let nch: Vec<char> = name.chars().collect();
                    if nch.first() == Some(&'.') && !allow_dot {
                        continue;
                    }
                    if match_glob_toks(&toks, &nch) {
                        names.push(name);
                    }
                }
                names.sort();
                for name in names {
                    next.push(join_glob(base, &name));
                }
            } else {
                let joined = join_glob(base, &comp_literal);
                if std::path::Path::new(&joined).exists() {
                    next.push(joined);
                }
            }
        }
        cands = next;
    }
    cands
}

/// Join a base path and a component with a single `/` separator, preserving a
/// leading-`/` (absolute) base and cwd-relative (empty) base.
fn join_glob(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base == "/" {
        format!("/{name}")
    } else if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// Whether every part of a word is quoted (single- or double-quoted). Used by
/// `[[ … == … ]]` to decide whether the right-hand side is a literal string
/// (fully quoted) or a glob pattern (any unquoted part).
fn word_is_all_quoted(w: &Word) -> bool {
    !w.parts.is_empty()
        && w.parts
            .iter()
            .all(|p| matches!(p, WordPart::SingleQuoted(_) | WordPart::DoubleQuoted(_)))
}

/// Match `text` against a shell glob `pattern` (`*`, `?`, `[...]`), anchored at
/// both ends (as `case` patterns and `[[ … == … ]]` require). Uses iterative
/// star-backtracking so it runs in linear space and near-linear time.
fn glob_match(pattern: &[char], text: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    // Last '*' position in the pattern and the text index it was matched at, so
    // we can backtrack and let the star consume one more character.
    let mut star: Option<(usize, usize)> = None;
    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
            continue;
        }
        let m = if pi < pattern.len() {
            glob_match_one(pattern, pi, text[ti])
        } else {
            None
        };
        match m {
            Some((true, next)) => {
                pi = next;
                ti += 1;
            }
            _ => {
                if let Some((sp, st)) = star {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                } else {
                    return false;
                }
            }
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Match a single non-`*` pattern element at `pi` against `ch`. Returns
/// `(matched, index-after-the-element)`, or `None` if the pattern is exhausted.
fn glob_match_one(pattern: &[char], pi: usize, ch: char) -> Option<(bool, usize)> {
    match pattern.get(pi)? {
        '?' => Some((true, pi + 1)),
        '[' => Some(glob_match_class(pattern, pi, ch)),
        c => Some((*c == ch, pi + 1)),
    }
}

/// Match a `[...]` character class starting at `pattern[pi] == '['`. Supports
/// ranges (`a-z`) and a leading `!`/`^` negation. An unterminated class is
/// treated as a literal `[`.
fn glob_match_class(pattern: &[char], pi: usize, ch: char) -> (bool, usize) {
    let mut i = pi + 1;
    let mut negate = false;
    if matches!(pattern.get(i), Some('!' | '^')) {
        negate = true;
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < pattern.len() {
        let c = pattern[i];
        if c == ']' && !first {
            return (matched ^ negate, i + 1);
        }
        first = false;
        if i + 2 < pattern.len() && pattern[i + 1] == '-' && pattern[i + 2] != ']' {
            if pattern[i] <= ch && ch <= pattern[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if c == ch {
                matched = true;
            }
            i += 1;
        }
    }
    // Unterminated: treat the '[' literally.
    (ch == '[', pi + 1)
}

/// Longest match of `pattern` starting at `text[start]`; returns the end index
/// (exclusive) of the match, or `None`. Used by `${…/…/…}` substitution.
fn glob_match_at(pattern: &[char], text: &[char], start: usize) -> Option<usize> {
    for j in (start..=text.len()).rev() {
        if glob_match(pattern, &text[start..j]) {
            return Some(j);
        }
    }
    None
}

/// `${name#pat}` / `${name##pat}` / `${name%pat}` / `${name%%pat}`.
fn param_trim(value: &str, pattern: &[char], suffix: bool, longest: bool) -> String {
    let v: Vec<char> = value.chars().collect();
    if suffix {
        // Remove a matching suffix `v[k..]`, keeping `v[..k]`. Shortest match =
        // largest k; longest match = smallest k.
        let range: Vec<usize> = if longest {
            (0..=v.len()).collect()
        } else {
            (0..=v.len()).rev().collect()
        };
        for k in range {
            if glob_match(pattern, &v[k..]) {
                return v[..k].iter().collect();
            }
        }
    } else {
        // Remove a matching prefix `v[..k]`, keeping `v[k..]`. Shortest match =
        // smallest k; longest match = largest k.
        let range: Vec<usize> = if longest {
            (0..=v.len()).rev().collect()
        } else {
            (0..=v.len()).collect()
        };
        for k in range {
            if glob_match(pattern, &v[..k]) {
                return v[k..].iter().collect();
            }
        }
    }
    value.to_string()
}

/// `${name:offset[:length]}` — a negative offset counts from the end; a negative
/// length is an offset from the end.
fn param_substr(value: &str, offset: i64, length: Option<i64>) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len() as i64;
    let mut start = offset;
    if start < 0 {
        start += n;
    }
    start = start.clamp(0, n);
    let end = match length {
        None => n,
        Some(len) if len < 0 => (n + len).max(start),
        Some(len) => (start + len).min(n),
    };
    let end = end.clamp(start, n);
    chars[start as usize..end as usize].iter().collect()
}

/// `${name/pat/repl}` and friends.
fn param_replace(
    value: &str,
    pattern: &[char],
    replacement: &str,
    all: bool,
    anchor: ReplaceAnchor,
) -> String {
    let v: Vec<char> = value.chars().collect();
    match anchor {
        ReplaceAnchor::Start => {
            if let Some(end) = glob_match_at(pattern, &v, 0) {
                let mut s = replacement.to_string();
                s.extend(v[end..].iter());
                return s;
            }
            value.to_string()
        }
        ReplaceAnchor::End => {
            for i in 0..=v.len() {
                if glob_match(pattern, &v[i..]) {
                    let mut s: String = v[..i].iter().collect();
                    s.push_str(replacement);
                    return s;
                }
            }
            value.to_string()
        }
        ReplaceAnchor::None => {
            let mut result = String::new();
            let mut i = 0;
            let mut done = false;
            while i < v.len() {
                let can_replace = !done || all;
                if can_replace
                    && let Some(end) = glob_match_at(pattern, &v, i)
                    && end > i
                {
                    result.push_str(replacement);
                    i = end;
                    done = true;
                    continue;
                }
                result.push(v[i]);
                i += 1;
            }
            result
        }
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        ":" | "true"
            | "false"
            | "cd"
            | "pwd"
            | "echo"
            | "printf"
            | "export"
            | "unset"
            | "set"
            | "shift"
            | "read"
            | "test"
            | "["
            | "eval"
            | "source"
            | "."
            | "type"
            | "exit"
            | "return"
            | "break"
            | "continue"
    )
}

fn open_out(path: &str, append: bool) -> io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true);
    if append {
        opts.append(true);
    } else {
        opts.truncate(true);
    }
    opts.open(path)
}

fn read_one_line<R: BufRead>(r: &mut R) -> Option<String> {
    let mut line = String::new();
    let n = r.read_line(&mut line).ok()?;
    if n == 0 {
        return None;
    }
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Some(line)
}

/// Split a string on the default IFS (whitespace), dropping empty fields.
fn split_ifs(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// Minimal `printf`: handles `%s`, `%d`, `%%`, and common backslash escapes.
fn format_printf(fmt: &str, args: &[String]) -> String {
    let mut out = String::new();
    let mut arg_i = 0;
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '%' => match chars.next() {
                Some('%') => out.push('%'),
                Some('s') => {
                    out.push_str(args.get(arg_i).map_or("", String::as_str));
                    arg_i += 1;
                }
                Some('d') => {
                    let n = args
                        .get(arg_i)
                        .and_then(|s| s.trim().parse::<i64>().ok())
                        .unwrap_or(0);
                    out.push_str(&n.to_string());
                    arg_i += 1;
                }
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            },
            other => out.push(other),
        }
    }
    out
}

/// Evaluate a `test`/`[` expression. Returns the boolean result (true = success).
fn eval_test(a: &[&str]) -> bool {
    match a.len() {
        0 => false,
        1 => !a[0].is_empty(),
        2 => {
            // Unary operator.
            let (op, x) = (a[0], a[1]);
            if op == "!" {
                return x.is_empty();
            }
            eval_unary(op, x)
        }
        3 => {
            let (l, op, r) = (a[0], a[1], a[2]);
            if op == "!" {
                // `! op x` handled as negation of a 2-arg test.
                return !eval_test(&a[1..]);
            }
            eval_binary(l, op, r)
        }
        _ => {
            // Handle a leading `!`; otherwise fall back to the first 3 args.
            if a[0] == "!" {
                !eval_test(&a[1..])
            } else {
                eval_binary(a[0], a[1], a[2])
            }
        }
    }
}

fn eval_unary(op: &str, x: &str) -> bool {
    match op {
        "-z" => x.is_empty(),
        "-n" => !x.is_empty(),
        "-e" => std::path::Path::new(x).exists(),
        "-f" => std::path::Path::new(x).is_file(),
        "-d" => std::path::Path::new(x).is_dir(),
        "-s" => std::fs::metadata(x).map(|m| m.len() > 0).unwrap_or(false),
        "-r" | "-w" | "-x" => std::path::Path::new(x).exists(),
        _ => !x.is_empty(),
    }
}

fn eval_binary(l: &str, op: &str, r: &str) -> bool {
    match op {
        "=" | "==" => l == r,
        "!=" => l != r,
        "<" => l < r,
        ">" => l > r,
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            let (Ok(a), Ok(b)) = (l.parse::<i64>(), r.parse::<i64>()) else {
                return false;
            };
            match op {
                "-eq" => a == b,
                "-ne" => a != b,
                "-lt" => a < b,
                "-le" => a <= b,
                "-gt" => a > b,
                "-ge" => a >= b,
                _ => false,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> (String, i32) {
        // Capture stdout by running through command-substitution-style capture.
        let mut sh = Shell::new();
        let mut buf = Vec::new();
        let prog = parse(src).expect("parse");
        {
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out);
        }
        (String::from_utf8_lossy(&buf).into_owned(), sh.last_status)
    }

    #[test]
    fn echo_and_status() {
        let (o, s) = run("echo hello");
        assert_eq!(o, "hello\n");
        assert_eq!(s, 0);
    }

    #[test]
    fn variables_and_expansion() {
        let (o, _) = run("x=world; echo hi $x");
        assert_eq!(o, "hi world\n");
    }

    #[test]
    fn param_default() {
        let (o, _) = run("echo ${undefined:-fallback}");
        assert_eq!(o, "fallback\n");
    }

    #[test]
    fn arithmetic() {
        let (o, _) = run("echo $((6 * 7))");
        assert_eq!(o, "42\n");
    }

    #[test]
    fn command_substitution() {
        let (o, _) = run("echo [$(echo inner)]");
        assert_eq!(o, "[inner]\n");
    }

    #[test]
    fn if_true() {
        let (o, _) = run("if true; then echo yes; else echo no; fi");
        assert_eq!(o, "yes\n");
    }

    #[test]
    fn for_loop() {
        let (o, _) = run("for x in a b c; do echo $x; done");
        assert_eq!(o, "a\nb\nc\n");
    }

    #[test]
    fn while_with_break() {
        let (o, _) = run("x=0; while true; do echo $x; x=$((x+1)); if [ $x -ge 3 ]; then break; fi; done");
        assert_eq!(o, "0\n1\n2\n");
    }

    #[test]
    fn and_or() {
        let (o, _) = run("true && echo a; false || echo b; false && echo c");
        assert_eq!(o, "a\nb\n");
    }

    #[test]
    fn function_call() {
        let (o, _) = run("greet() { echo hi $1; }; greet there");
        assert_eq!(o, "hi there\n");
    }

    #[test]
    fn test_builtin() {
        let (_, s) = run("[ 3 -gt 2 ]");
        assert_eq!(s, 0);
        let (_, s2) = run("[ 1 -gt 2 ]");
        assert_eq!(s2, 1);
    }

    #[test]
    fn length_expansion() {
        let (o, _) = run("x=hello; echo ${#x}");
        assert_eq!(o, "5\n");
    }

    #[test]
    fn negated_pipeline_status() {
        let (_, s) = run("! true");
        assert_eq!(s, 1);
    }

    #[test]
    fn quoted_no_split() {
        let (o, _) = run(r#"x="a b c"; for w in "$x"; do echo $w; done"#);
        assert_eq!(o, "a b c\n");
    }

    #[test]
    fn case_literal_and_glob() {
        let (o, _) = run("case hello in h*) echo star;; *) echo other;; esac");
        assert_eq!(o, "star\n");
        let (o2, _) = run("case foo in a|foo|b) echo alt;; esac");
        assert_eq!(o2, "alt\n");
        let (o3, _) = run("case xyz in a*) echo a;; esac; echo done");
        assert_eq!(o3, "done\n");
    }

    #[test]
    fn case_uses_variable() {
        let (o, _) = run("x=cat.txt; case $x in *.txt) echo text;; *.md) echo md;; esac");
        assert_eq!(o, "text\n");
    }

    #[test]
    fn case_char_class() {
        let (o, _) = run("case 5 in [0-9]) echo digit;; *) echo no;; esac");
        assert_eq!(o, "digit\n");
    }

    #[test]
    fn here_string_read() {
        let (o, _) = run("read x <<< hello; echo got $x");
        assert_eq!(o, "got hello\n");
    }

    #[test]
    fn here_doc_read_and_expand() {
        let (o, _) = run("name=world\nread line <<EOF\nhi $name\nEOF\necho $line");
        assert_eq!(o, "hi world\n");
    }

    #[test]
    fn here_doc_quoted_delim_no_expand() {
        let (o, _) = run("name=world\nread line <<'EOF'\nhi $name\nEOF\necho $line");
        assert_eq!(o, "hi $name\n");
    }

    #[test]
    fn param_trim_prefix_suffix() {
        assert_eq!(run("x=foo.tar.gz; echo ${x#*.}").0, "tar.gz\n");
        assert_eq!(run("x=foo.tar.gz; echo ${x##*.}").0, "gz\n");
        assert_eq!(run("x=foo.tar.gz; echo ${x%.*}").0, "foo.tar\n");
        assert_eq!(run("x=foo.tar.gz; echo ${x%%.*}").0, "foo\n");
    }

    #[test]
    fn param_substring() {
        assert_eq!(run("x=abcdef; echo ${x:2}").0, "cdef\n");
        assert_eq!(run("x=abcdef; echo ${x:2:3}").0, "cde\n");
        assert_eq!(run("x=abcdef; echo ${x: -2}").0, "ef\n");
        assert_eq!(run("x=abcdef; echo ${x:1:-1}").0, "bcde\n");
    }

    #[test]
    fn param_replace_forms() {
        assert_eq!(run("x=aXbXc; echo ${x/X/-}").0, "a-bXc\n");
        assert_eq!(run("x=aXbXc; echo ${x//X/-}").0, "a-b-c\n");
        assert_eq!(run("x=abcabc; echo ${x/#abc/Z}").0, "Zabc\n");
        assert_eq!(run("x=abcabc; echo ${x/%abc/Z}").0, "abcZ\n");
        assert_eq!(run("x=hello; echo ${x//l/}").0, "heo\n");
    }

    #[test]
    fn param_ops_still_work() {
        assert_eq!(run("echo ${u:-default}").0, "default\n");
        assert_eq!(run("x=set; echo ${x:+yes}").0, "yes\n");
    }

    #[test]
    fn glob_match_basics() {
        let g = |p: &str, t: &str| glob_match(&p.chars().collect::<Vec<_>>(), &t.chars().collect::<Vec<_>>());
        assert!(g("*", "anything"));
        assert!(g("h?llo", "hello"));
        assert!(g("a*c", "abbbc"));
        assert!(!g("a*c", "abbb"));
        assert!(g("[a-c]x", "bx"));
        assert!(!g("[a-c]x", "dx"));
        assert!(g("[!0-9]", "z"));
        assert!(!g("[!0-9]", "5"));
        assert!(g("file.txt", "file.txt"));
        assert!(!g("file.txt", "file.md"));
    }

    #[test]
    fn cond_string_equality() {
        assert_eq!(run("[[ foo == foo ]]").1, 0);
        assert_eq!(run("[[ foo == bar ]]").1, 1);
        assert_eq!(run("[[ foo != bar ]]").1, 0);
        assert_eq!(run("x=hello; [[ $x = hello ]]").1, 0);
    }

    #[test]
    fn cond_glob_and_quoting() {
        // Unquoted RHS is a glob pattern.
        assert_eq!(run("[[ foobar == foo* ]]").1, 0);
        assert_eq!(run("[[ foobar == baz* ]]").1, 1);
        // Quoted RHS is a literal, so the `*` does not match.
        assert_eq!(run("[[ foobar == \"foo*\" ]]").1, 1);
        assert_eq!(run("[[ 'foo*' == \"foo*\" ]]").1, 0);
    }

    #[test]
    fn cond_numeric() {
        assert_eq!(run("[[ 3 -gt 2 ]]").1, 0);
        assert_eq!(run("[[ 2 -gt 3 ]]").1, 1);
        assert_eq!(run("[[ 5 -eq 5 ]]").1, 0);
        assert_eq!(run("x=4; [[ $x -le 4 ]]").1, 0);
        // Operands undergo arithmetic evaluation.
        assert_eq!(run("[[ 2+2 -eq 4 ]]").1, 0);
    }

    #[test]
    fn cond_string_len() {
        assert_eq!(run("[[ -z \"\" ]]").1, 0);
        assert_eq!(run("[[ -n foo ]]").1, 0);
        assert_eq!(run("x=; [[ -z $x ]]").1, 0);
        assert_eq!(run("x=set; [[ -n $x ]]").1, 0);
    }

    #[test]
    fn cond_logical_and_grouping() {
        assert_eq!(run("[[ 1 -eq 1 && 2 -eq 2 ]]").1, 0);
        assert_eq!(run("[[ 1 -eq 1 && 2 -eq 3 ]]").1, 1);
        assert_eq!(run("[[ 1 -eq 2 || 3 -eq 3 ]]").1, 0);
        assert_eq!(run("[[ ! 1 -eq 2 ]]").1, 0);
        assert_eq!(run("[[ ( 1 -eq 1 || 1 -eq 2 ) && 3 -eq 3 ]]").1, 0);
    }

    #[test]
    fn cond_string_ordering() {
        assert_eq!(run("[[ abc < abd ]]").1, 0);
        assert_eq!(run("[[ abd < abc ]]").1, 1);
        assert_eq!(run("[[ b > a ]]").1, 0);
    }

    #[test]
    fn cond_in_if() {
        assert_eq!(run("if [[ foo == foo ]]; then echo yes; fi").0, "yes\n");
        assert_eq!(
            run("x=3; if [[ $x -gt 5 ]]; then echo big; else echo small; fi").0,
            "small\n"
        );
    }

    #[test]
    fn arith_command_status() {
        assert_eq!(run("(( 1 + 1 ))").1, 0);
        assert_eq!(run("(( 0 ))").1, 1);
        assert_eq!(run("(( 5 > 3 ))").1, 0);
        assert_eq!(run("(( 3 > 5 ))").1, 1);
    }

    #[test]
    fn arith_command_with_vars() {
        assert_eq!(run("x=4; (( x > 3 ))").1, 0);
        assert_eq!(run("x=2; (( x > 3 ))").1, 1);
        // Used as a condition.
        assert_eq!(run("x=10; if (( x > 5 )); then echo big; fi").0, "big\n");
    }

    #[test]
    fn nested_subshell_still_works() {
        // `( ( … ) )` with an inner space is nested subshells, not arithmetic.
        assert_eq!(run("( ( echo hi ) )").0, "hi\n");
    }

    fn field_lit(s: &str) -> Vec<EChar> {
        s.chars().map(|c| EChar { c, quoted: false }).collect()
    }

    #[test]
    fn glob_toks_match() {
        let f = |p: &str, n: &str| {
            match_glob_toks(&compile_glob(&field_lit(p)), &n.chars().collect::<Vec<_>>())
        };
        assert!(f("*.txt", "a.txt"));
        assert!(!f("*.txt", "a.log"));
        assert!(f("h?llo", "hello"));
        assert!(f("[ab]?", "a1"));
        assert!(!f("[ab]?", "c1"));
        assert!(f("[!0-9]x", "zx"));
        assert!(!f("[!0-9]x", "5x"));
    }

    #[test]
    fn glob_quoted_metachar_is_literal() {
        // A quoted `*` is a literal star, never a pattern.
        let mut field = field_lit("");
        field.push(EChar { c: '*', quoted: true });
        let toks = compile_glob(&field);
        assert!(match_glob_toks(&toks, &['*']));
        assert!(!match_glob_toks(&toks, &['a']));
    }

    #[test]
    fn glob_filesystem_expansion() {
        // Use a uniquely-named cwd-relative dir to avoid the process-wide-cwd
        // race between parallel tests (no `set_current_dir`).
        let uniq = format!(
            "osh_globtest_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let dir = std::path::Path::new(&uniq);
        std::fs::create_dir_all(dir).expect("mkdir");
        for n in ["a.txt", "b.txt", "c.log", ".hidden"] {
            std::fs::File::create(dir.join(n)).expect("touch");
        }

        let basename = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();

        // `*.txt` matches the two text files (sorted), not the log or hidden.
        let mut txt: Vec<String> = glob_expand_field(&field_lit(&format!("{uniq}/*.txt")))
            .iter()
            .map(|p| basename(p))
            .collect();
        txt.sort();
        assert_eq!(txt, vec!["a.txt".to_string(), "b.txt".to_string()]);

        // `*` honors the leading-dot rule (no `.hidden`).
        let all = glob_expand_field(&field_lit(&format!("{uniq}/*")));
        assert!(all.iter().all(|p| !p.ends_with(".hidden")));
        assert_eq!(all.len(), 3);

        // An explicit leading `.` matches hidden files.
        let dot = glob_expand_field(&field_lit(&format!("{uniq}/.*")));
        assert!(dot.iter().any(|p| p.ends_with(".hidden")));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn glob_no_match_stays_literal() {
        // With no match and no `nullglob`, the pattern is left as the word.
        assert_eq!(run("echo osh_definitely_no_such_glob_*.zzz").0, "osh_definitely_no_such_glob_*.zzz\n");
    }

    #[test]
    fn array_literal_and_index() {
        assert_eq!(run("a=(one two three); echo ${a[0]}").0, "one\n");
        assert_eq!(run("a=(one two three); echo ${a[2]}").0, "three\n");
        // A bare reference reads element 0.
        assert_eq!(run("a=(one two three); echo $a").0, "one\n");
        // Out-of-range index expands to empty.
        assert_eq!(run("a=(one two); echo x${a[9]}y").0, "xy\n");
    }

    #[test]
    fn array_all_and_star() {
        assert_eq!(run("a=(x y z); echo ${a[@]}").0, "x y z\n");
        assert_eq!(run("a=(x y z); echo ${a[*]}").0, "x y z\n");
    }

    #[test]
    fn array_length() {
        assert_eq!(run("a=(a b c d); echo ${#a[@]}").0, "4\n");
        assert_eq!(run("a=(a b c d); echo ${#a[*]}").0, "4\n");
        // Length of a specific element.
        assert_eq!(run("a=(hi hello); echo ${#a[1]}").0, "5\n");
    }

    #[test]
    fn array_indexed_assignment() {
        assert_eq!(run("a=(x y z); a[1]=Q; echo ${a[@]}").0, "x Q z\n");
        // Assigning past the end grows the array with empty slots.
        assert_eq!(run("a=(x); a[3]=w; echo ${#a[@]}").0, "4\n");
    }

    #[test]
    fn array_append() {
        assert_eq!(run("a=(x y); a+=(z w); echo ${a[@]}").0, "x y z w\n");
        assert_eq!(run("a=(x y); a+=(z); echo ${#a[@]}").0, "3\n");
    }

    #[test]
    fn array_quoted_all_preserves_fields() {
        // "${a[@]}" keeps element boundaries even with embedded spaces.
        let out = run(r#"a=("a b" c); for w in "${a[@]}"; do echo "[$w]"; done"#).0;
        assert_eq!(out, "[a b]\n[c]\n");
    }

    #[test]
    fn array_unquoted_all_splits() {
        // Unquoted ${a[@]} field-splits, so "a b" becomes two words.
        let out = run(r#"a=("a b" c); for w in ${a[@]}; do echo "[$w]"; done"#).0;
        assert_eq!(out, "[a]\n[b]\n[c]\n");
    }

    #[test]
    fn array_unset() {
        assert_eq!(run("a=(x y z); unset a[1]; echo ${a[@]}").0, "x z\n");
        assert_eq!(run("a=(x y z); unset a; echo ${#a[@]}").0, "0\n");
    }

    #[test]
    fn array_from_glob_and_expansion() {
        // Array elements undergo splitting/expansion.
        assert_eq!(run("s='p q'; a=($s r); echo ${#a[@]}").0, "3\n");
    }
}
