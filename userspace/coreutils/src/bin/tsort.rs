//! tsort — topological sort.
//!
//! Usage: tsort [FILE]
//!   Read pairs of strings from FILE (or stdin if omitted) representing
//!   directed edges in a graph, and output the nodes in topological order.
//!   Detects and reports cycles.
//!
//! Input format: whitespace-separated pairs, one edge per pair.
//!   a b     means "a must come before b"
//!   c c     means "c exists" (self-edge, just adds the node)
//!
//! Exit codes:
//!   0  success
//!   1  cycle detected (partial output is still produced)

use std::collections::{HashMap, VecDeque};
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

/// Tokenise input by whitespace.  Returns an error if the token count is odd
/// (tsort consumes pairs of tokens as edges).
fn tokenise(input: &str) -> Result<Vec<String>, String> {
    let tokens: Vec<String> = input.split_whitespace().map(str::to_string).collect();
    if !tokens.len().is_multiple_of(2) {
        return Err("odd number of tokens".to_string());
    }
    Ok(tokens)
}

/// Build the graph from a flat list of token pairs.  Returns
/// (id_to_name, adjacency, in_degree).  Self-loops (`x x`) are recorded as
/// vertices but produce no edges.
fn build_graph(tokens: &[String]) -> (Vec<String>, Vec<Vec<usize>>, Vec<usize>) {
    let mut name_to_id: HashMap<String, usize> = HashMap::new();
    let mut id_to_name: Vec<String> = Vec::new();

    let mut get_id = |name: &str| -> usize {
        if let Some(&id) = name_to_id.get(name) {
            id
        } else {
            let id = id_to_name.len();
            name_to_id.insert(name.to_string(), id);
            id_to_name.push(name.to_string());
            id
        }
    };

    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut i: usize = 0;
    while i.saturating_add(1) < tokens.len() {
        let Some(t_a) = tokens.get(i) else { break };
        let Some(t_b) = tokens.get(i.saturating_add(1)) else {
            break;
        };
        let a = get_id(t_a);
        let b = get_id(t_b);
        if a != b {
            edges.push((a, b));
        }
        i = i.saturating_add(2);
    }

    let n = id_to_name.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];

    for &(from, to) in &edges {
        if let Some(row) = adj.get_mut(from) {
            row.push(to);
        }
        if let Some(d) = in_degree.get_mut(to) {
            *d = d.saturating_add(1);
        }
    }

    (id_to_name, adj, in_degree)
}

/// Run Kahn's algorithm.  Returns `(emitted, remaining)` where `emitted` is the
/// topological order and `remaining` lists the node IDs (in numerical order)
/// that were never emitted — i.e. the cycle members.
fn topological_sort(adj: &[Vec<usize>], in_degree: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let n = adj.len();
    let mut deg: Vec<usize> = in_degree.to_vec();
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (node, &d) in deg.iter().enumerate() {
        if d == 0 {
            queue.push_back(node);
        }
    }

    let mut emitted: Vec<usize> = Vec::new();
    while let Some(node) = queue.pop_front() {
        emitted.push(node);
        if let Some(neighbours) = adj.get(node) {
            for &neighbor in neighbours {
                if let Some(d) = deg.get_mut(neighbor) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    let mut remaining: Vec<usize> = Vec::new();
    if emitted.len() < n {
        for (node, &d) in deg.iter().enumerate() {
            if d > 0 {
                remaining.push(node);
            }
        }
    }

    (emitted, remaining)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let reader: Box<dyn Read> = if args.is_empty() || args.first().map(String::as_str) == Some("-") {
        Box::new(io::stdin())
    } else {
        let path = args.first().map(String::as_str).unwrap_or("-");
        match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tsort: {path}: {e}");
                process::exit(1);
            }
        }
    };

    // Concatenate all lines, then tokenise as one input.
    let buf = BufReader::new(reader);
    let mut input = String::new();
    for line in buf.lines() {
        match line {
            Ok(l) => {
                input.push_str(&l);
                input.push('\n');
            }
            Err(e) => {
                eprintln!("tsort: {e}");
                process::exit(1);
            }
        }
    }

    let tokens = match tokenise(&input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("tsort: {e}");
            process::exit(1);
        }
    };

    let (names, adj, in_degree) = build_graph(&tokens);
    let (emitted, remaining) = topological_sort(&adj, &in_degree);

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for node in &emitted {
        if let Some(name) = names.get(*node) {
            let _ = writeln!(out, "{name}");
        }
    }

    if !remaining.is_empty() {
        eprintln!("tsort: input contains a cycle");
        for node in &remaining {
            if let Some(name) = names.get(*node) {
                let _ = writeln!(out, "{name}");
            }
        }
        process::exit(1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- tokenise ----------------

    #[test]
    fn tokenise_empty_ok() {
        assert_eq!(tokenise("").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn tokenise_pair() {
        assert_eq!(tokenise("a b").unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn tokenise_multiline() {
        let toks = tokenise("a b\nc d\n").unwrap();
        assert_eq!(toks, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn tokenise_odd_errors() {
        assert!(tokenise("a b c").is_err());
    }

    #[test]
    fn tokenise_extra_whitespace_collapsed() {
        let toks = tokenise("  a   b  \n\n  c  d \t e f").unwrap();
        assert_eq!(toks, vec!["a", "b", "c", "d", "e", "f"]);
    }

    // ---------------- build_graph ----------------

    #[test]
    fn graph_simple_edge() {
        let toks = vec!["a".to_string(), "b".to_string()];
        let (names, adj, in_deg) = build_graph(&toks);
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(adj[0], vec![1]);
        assert!(adj[1].is_empty());
        assert_eq!(in_deg, vec![0, 1]);
    }

    #[test]
    fn graph_self_loop_adds_node_no_edge() {
        let toks = vec!["x".to_string(), "x".to_string()];
        let (names, adj, in_deg) = build_graph(&toks);
        assert_eq!(names, vec!["x"]);
        assert!(adj[0].is_empty());
        assert_eq!(in_deg, vec![0]);
    }

    #[test]
    fn graph_diamond() {
        // a -> b, a -> c, b -> d, c -> d.
        let toks: Vec<String> = ["a", "b", "a", "c", "b", "d", "c", "d"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let (names, adj, in_deg) = build_graph(&toks);
        assert_eq!(names, vec!["a", "b", "c", "d"]);
        // a (id 0) -> b (1), c (2)
        assert_eq!(adj[0], vec![1, 2]);
        // b (1) -> d (3)
        assert_eq!(adj[1], vec![3]);
        // c (2) -> d (3)
        assert_eq!(adj[2], vec![3]);
        // d (3) has no outgoing.
        assert!(adj[3].is_empty());
        assert_eq!(in_deg, vec![0, 1, 1, 2]);
    }

    // ---------------- topological_sort ----------------

    #[test]
    fn sort_empty_graph() {
        let (emitted, remaining) = topological_sort(&[], &[]);
        assert!(emitted.is_empty());
        assert!(remaining.is_empty());
    }

    #[test]
    fn sort_linear_chain() {
        // 0 -> 1 -> 2.
        let adj = vec![vec![1], vec![2], vec![]];
        let in_deg = vec![0, 1, 1];
        let (emitted, remaining) = topological_sort(&adj, &in_deg);
        assert_eq!(emitted, vec![0, 1, 2]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn sort_diamond_topo_order_valid() {
        // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3.
        let adj = vec![vec![1, 2], vec![3], vec![3], vec![]];
        let in_deg = vec![0, 1, 1, 2];
        let (emitted, remaining) = topological_sort(&adj, &in_deg);
        assert_eq!(emitted.len(), 4);
        assert!(remaining.is_empty());
        // First emitted must be node 0; last must be node 3.
        assert_eq!(*emitted.first().unwrap(), 0);
        assert_eq!(*emitted.last().unwrap(), 3);
    }

    #[test]
    fn sort_cycle_emits_remaining() {
        // 0 -> 1 -> 0 (cycle).
        let adj = vec![vec![1], vec![0]];
        let in_deg = vec![1, 1];
        let (emitted, remaining) = topological_sort(&adj, &in_deg);
        assert!(emitted.is_empty());
        assert_eq!(remaining, vec![0, 1]);
    }

    #[test]
    fn sort_partial_cycle() {
        // 0 -> 1, 1 -> 2 -> 3 -> 2 (cycle on 2,3).  Only 0,1 should emit.
        let adj = vec![vec![1], vec![2], vec![3], vec![2]];
        let in_deg = vec![0, 1, 2, 1];
        let (emitted, remaining) = topological_sort(&adj, &in_deg);
        assert_eq!(emitted, vec![0, 1]);
        assert_eq!(remaining, vec![2, 3]);
    }

    #[test]
    fn sort_disconnected_components_both_emit() {
        // 0 -> 1, 2 -> 3 (two separate edges).
        let adj = vec![vec![1], vec![], vec![3], vec![]];
        let in_deg = vec![0, 1, 0, 1];
        let (emitted, remaining) = topological_sort(&adj, &in_deg);
        assert_eq!(emitted.len(), 4);
        assert!(remaining.is_empty());
    }
}
