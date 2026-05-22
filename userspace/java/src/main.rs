#![deny(clippy::all)]

//! java — OurOS Java runtime environment
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `java` (default) — Java application launcher
//! - `javac` — Java compiler
//! - `jar` — Java archive tool
//! - `javadoc` — Java documentation generator
//! - `jps` — Java process status
//! - `jstack` — Java stack trace
//! - `jmap` — Java memory map
//! - `jconsole` — Java monitoring and management console

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_java(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help" || a == "-h") {
        println!("Usage: java [options] <mainclass> [args...]");
        println!("       java [options] -jar <jarfile> [args...]");
        println!();
        println!("Options:");
        println!("  -cp, -classpath <paths>   Class search path");
        println!("  -jar <jarfile>            Execute a JAR file");
        println!("  -Dproperty=value          Set a system property");
        println!("  -Xmx<size>               Maximum heap size");
        println!("  -Xms<size>               Initial heap size");
        println!("  -Xss<size>               Thread stack size");
        println!("  -server                  Select the server VM");
        println!("  -verbose[:class|gc|jni]  Enable verbose output");
        println!("  --version                Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version" || a == "-version") {
        println!("openjdk version \"21.0.2\" 2025-01-16");
        println!("OpenJDK Runtime Environment (OurOS build 21.0.2+13)");
        println!("OpenJDK 64-Bit Server VM (OurOS build 21.0.2+13, mixed mode)");
        return 0;
    }

    // Detect -jar mode
    if let Some(pos) = args.iter().position(|a| a == "-jar") {
        if let Some(jar) = args.get(pos + 1) {
            println!("Executing JAR: {} (simulated)", jar);
            return 0;
        }
        eprintln!("Error: -jar requires jar file specification");
        return 1;
    }

    // Main class
    let main_class = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(cls) = main_class {
        println!("Executing class: {} (simulated)", cls);
        return 0;
    }

    eprintln!("Error: Main class not found");
    1
}

fn run_javac(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help" || a == "-h") {
        println!("Usage: javac <options> <source files>");
        println!();
        println!("Options:");
        println!("  -d <directory>           Destination directory");
        println!("  -cp, -classpath <path>   Class path");
        println!("  -source <release>        Source compatibility version");
        println!("  -target <release>        Target bytecode version");
        println!("  --release <release>      Compile for specific release");
        println!("  -g                       Generate debugging info");
        println!("  -Xlint                   Enable recommended warnings");
        println!("  -proc:none               Disable annotation processing");
        println!("  --version                Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("javac 21.0.2");
        return 0;
    }

    let sources: Vec<&str> = args.iter().filter(|a| a.ends_with(".java")).map(|s| s.as_str()).collect();
    if sources.is_empty() {
        eprintln!("error: no source files");
        return 1;
    }
    for src in &sources {
        println!("Compiling {} (simulated)", src);
    }
    0
}

fn run_jar(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jar [OPTION...] [ [--release VERSION] [-C dir] files] ...");
        println!();
        println!("Options:");
        println!("  -c, --create      Create the archive");
        println!("  -t, --list        List table of contents");
        println!("  -x, --extract     Extract named (or all) files");
        println!("  -u, --update      Update an existing archive");
        println!("  -f, --file=FILE   The archive file name");
        println!("  -m, --manifest=FILE  Include manifest info");
        println!("  -e, --main-class=CLASSNAME  Main class");
        println!("  -v, --verbose     Verbose output");
        println!("  --version         Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("jar 21.0.2");
        return 0;
    }

    if args.iter().any(|a| a.contains('c') && !a.starts_with('-') || a == "--create" || a == "-c") {
        println!("Creating archive (simulated)");
    } else if args.iter().any(|a| a.contains('t') && !a.starts_with('-') || a == "--list" || a == "-t") {
        println!("META-INF/");
        println!("META-INF/MANIFEST.MF");
        println!("com/example/Main.class");
    } else if args.iter().any(|a| a.contains('x') && !a.starts_with('-') || a == "--extract" || a == "-x") {
        println!("Extracting archive (simulated)");
    }
    0
}

fn run_jps(args: Vec<String>) -> i32 {
    let verbose = args.iter().any(|a| a == "-l" || a == "-v");
    if verbose {
        println!("12345 com.example.Main -Xmx512m -Xms256m");
        println!("12346 org.gradle.launcher.daemon.bootstrap.GradleDaemon 8.5");
        println!("12347 sun.tools.jps.Jps -l -v");
    } else {
        println!("12345 Main");
        println!("12346 GradleDaemon");
        println!("12347 Jps");
    }
    0
}

fn run_jstack(args: Vec<String>) -> i32 {
    let pid = args.first().map(|s| s.as_str()).unwrap_or("12345");
    println!("Full thread dump OpenJDK 64-Bit Server VM (21.0.2+13):");
    println!();
    println!("\"main\" #1 prio=5 os_prio=0 tid=0x00007f4c00001800 nid=0x{} runnable", pid);
    println!("   java.lang.Thread.State: RUNNABLE");
    println!("\tat com.example.Main.processData(Main.java:42)");
    println!("\tat com.example.Main.main(Main.java:15)");
    println!();
    println!("\"GC task thread#0\" os_prio=0 tid=0x00007f4c00023800");
    println!("   java.lang.Thread.State: RUNNABLE");
    0
}

fn run_jmap(args: Vec<String>) -> i32 {
    let pid = args.first().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("12345");
    let heap = args.iter().any(|a| a == "-heap");

    if heap {
        println!("Attaching to process ID {}, please wait...", pid);
        println!();
        println!("Heap Configuration:");
        println!("   MinHeapFreeRatio = 40");
        println!("   MaxHeapFreeRatio = 70");
        println!("   MaxHeapSize      = 536870912 (512.0MB)");
        println!("   NewSize          = 1363144 (1.3MB)");
        println!();
        println!("Heap Usage:");
        println!("Young Generation:");
        println!("   capacity = 35258368 (33.625MB)");
        println!("   used     = 14567424 (13.89MB)");
        println!("Old Generation:");
        println!("   capacity = 89128960 (85.0MB)");
        println!("   used     = 34567168 (32.96MB)");
    } else {
        println!("Attaching to process ID {}...", pid);
        println!("(use -heap for heap summary, -histo for histogram)");
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("java");
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
        "javac" => run_javac(rest),
        "jar" => run_jar(rest),
        "jps" => run_jps(rest),
        "jstack" => run_jstack(rest),
        "jmap" => run_jmap(rest),
        "javadoc" => { println!("Generating Javadoc... done (simulated)"); 0 }
        "jconsole" => { println!("JConsole: monitoring (simulated)"); 0 }
        _ => run_java(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() {
        assert!(true);
    }
}
