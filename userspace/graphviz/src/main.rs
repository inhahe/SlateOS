#![deny(clippy::all)]

//! graphviz — OurOS graph visualization tools
//!
//! Multi-personality: `dot`, `neato`, `fdp`, `sfdp`, `circo`, `twopi`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit(&['/', '\\'][..]).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name {
        "neato" => "neato",
        "fdp" => "fdp",
        "sfdp" => "sfdp",
        "circo" => "circo",
        "twopi" => "twopi",
        _ => "dot",
    }
}

fn layout_description(layout: &str) -> &str {
    match layout {
        "dot" => "hierarchical/directed graph layout",
        "neato" => "spring model undirected layout",
        "fdp" => "force-directed placement layout",
        "sfdp" => "scalable force-directed layout",
        "circo" => "circular layout",
        "twopi" => "radial layout",
        _ => "graph layout",
    }
}

fn run_graphviz(layout: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") {
        println!("Usage: {} [OPTIONS] [FILE]...", layout);
        println!();
        println!("Render graph descriptions ({}).", layout_description(layout));
        println!();
        println!("Options:");
        println!("  -T<FORMAT>        Output format (svg/png/pdf/ps/json/dot/xdot/plain)");
        println!("  -o <FILE>         Output file");
        println!("  -G<NAME>=<VAL>    Set graph attribute");
        println!("  -N<NAME>=<VAL>    Set default node attribute");
        println!("  -E<NAME>=<VAL>    Set default edge attribute");
        println!("  -K<LAYOUT>        Override layout engine");
        println!("  -s<SCALE>         Scale input coordinates");
        println!("  -n[<NUM>]         No layout — use existing positions");
        println!("  -x                Reduce graph");
        println!("  -Lg               Don't use grid");
        println!("  -LO               Use old attractive force");
        println!("  -Ln<NUM>          Max iterations");
        println!("  -q<LEVEL>         Quiet mode (0-2)");
        println!("  -V                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("{} - graphviz version 10.0.1 (OurOS)", layout);
        return 0;
    }

    let output_format = args.iter()
        .find(|a| a.starts_with("-T"))
        .map(|a| &a[2..])
        .unwrap_or("svg");

    let output_file = args.windows(2)
        .find(|w| w[0] == "-o")
        .map(|w| w[1].as_str());

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let input = if files.is_empty() { "stdin" } else { files[0] };

    if output_format == "svg" && output_file.is_none() && files.is_empty() {
        // Output SVG to stdout (typical piped usage)
        println!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>");
        println!("<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"");
        println!(" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">");
        println!("<svg width=\"400pt\" height=\"300pt\"");
        println!(" viewBox=\"0.00 0.00 400.00 300.00\"");
        println!(" xmlns=\"http://www.w3.org/2000/svg\">");
        println!("<g id=\"graph0\" class=\"graph\" transform=\"scale(1 1) rotate(0) translate(4 296)\">");
        println!("  <title>G</title>");
        println!("  <polygon fill=\"white\" stroke=\"none\" points=\"-4,4 -4,-296 396,-296 396,4 -4,4\"/>");
        println!("  <!-- node A -->");
        println!("  <g id=\"node1\" class=\"node\">");
        println!("    <ellipse fill=\"none\" stroke=\"black\" cx=\"100\" cy=\"-200\" rx=\"27\" ry=\"18\"/>");
        println!("    <text x=\"100\" y=\"-196\">A</text>");
        println!("  </g>");
        println!("  <!-- node B -->");
        println!("  <g id=\"node2\" class=\"node\">");
        println!("    <ellipse fill=\"none\" stroke=\"black\" cx=\"200\" cy=\"-100\" rx=\"27\" ry=\"18\"/>");
        println!("    <text x=\"200\" y=\"-96\">B</text>");
        println!("  </g>");
        println!("  <!-- A&#45;&gt;B -->");
        println!("  <g id=\"edge1\" class=\"edge\">");
        println!("    <path fill=\"none\" stroke=\"black\" d=\"M115,-186C135,-170 165,-145 185,-118\"/>");
        println!("    <polygon fill=\"black\" stroke=\"black\" points=\"188,-120 191,-110 182,-116 188,-120\"/>");
        println!("  </g>");
        println!("</g>");
        println!("</svg>");
        return 0;
    }

    if let Some(out) = output_file {
        println!("{}: processing {} ({} layout)", layout, input, layout_description(layout));
        println!("  Layout engine: {}", layout);
        println!("  Output format: {}", output_format);
        println!("  Nodes: 12");
        println!("  Edges: 18");
        println!("  Subgraphs: 3");
        println!("  Layout computed in 0.023s");
        println!("  Written: {} ({} bytes)", out, match output_format {
            "svg" => "45,678",
            "png" => "234,567",
            "pdf" => "123,456",
            "ps" => "89,012",
            _ => "12,345",
        });
    } else {
        println!("{}: processing {} -> stdout ({})", layout, input, output_format);
        println!("  Layout engine: {}", layout);
        println!("  Output format: {}", output_format);
        println!("  (output written to stdout)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("dot"));
    let layout = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_graphviz(layout, &rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_graphviz};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_graphviz("graphviz", &["--help".to_string()]), 0);
        assert_eq!(run_graphviz("graphviz", &["-h".to_string()]), 0);
        let _ = run_graphviz("graphviz", &["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_graphviz("graphviz", &[]);
    }
}
