//! Tests for the network widget: what it says of the link and the traffic,
//! where its lines and graph go, what it draws them in, and when the shell
//! reads the link.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use std::net::Ipv4Addr;

use super::*;
use crate::widgets::monitor::NetRate;
use appearance::palette_check::assert_drawn_from;

/// A veil no default gives a widget, so no wash is right by accident.
const ODD_ALPHA: u8 = 137;

/// QEMU's user network, up.
fn qemu() -> Link {
    Link {
        up: true,
        address: Some(Ipv4Addr::new(10, 0, 2, 15)),
        prefix: Some(24),
        gateway: Some(Ipv4Addr::new(10, 0, 2, 2)),
        dns: Some(Ipv4Addr::new(10, 0, 2, 3)),
        mac: "52:54:00:12:34:56".to_owned(),
    }
}

/// The traffic as the system monitor sampled it: measured, and moving.
fn measured() -> Graphs {
    Graphs {
        network: vec![2_000.0, 9_000.0, 8_000.0],
        network_now: Some(NetRate {
            down: 6_000,
            up: 2_000,
        }),
        ..Graphs::default()
    }
}

/// A reading of `link` with `graphs` for its traffic.
fn reading(link: LinkState, graphs: Graphs) -> NetworkReading {
    NetworkReading {
        link: Some(Ok(link)),
        graphs,
    }
}

/// A wide widget's content, one cell high.
fn wide() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 96.0)
}

/// A large widget's content.
fn large() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 258.0)
}

/// What `lines` says, as `(label, value)`.
fn said(reading: &NetworkReading) -> Vec<(&'static str, String)> {
    lines(reading)
        .into_iter()
        .map(|line| (line.label, line.value))
        .collect()
}

/// **A link that is up is said in full**: connected, the address with its
/// prefix, the traffic in and out, the gateway and the name server -- in
/// that order, most wanted first.
#[test]
fn a_link_that_is_up_is_said_in_full() {
    assert_eq!(
        said(&reading(LinkState::Link(qemu()), measured())),
        [
            ("Network", "Connected".to_owned()),
            ("Address", "10.0.2.15/24".to_owned()),
            ("Traffic", "6.0 kB/s in, 2.0 kB/s out".to_owned()),
            ("Gateway", "10.0.2.2".to_owned()),
            ("DNS", "10.0.2.3".to_owned()),
        ]
    );
}

/// **What is not there is said so**: a link down with no address yet, its
/// traffic not measured, no gateway or name server.
#[test]
fn what_is_not_there_is_said_so() {
    let bare = Link {
        up: false,
        address: None,
        prefix: None,
        gateway: None,
        dns: None,
        ..qemu()
    };
    assert_eq!(
        said(&reading(LinkState::Link(bare), Graphs::default())),
        [
            ("Network", "Not connected".to_owned()),
            ("Address", "None yet".to_owned()),
            ("Traffic", NOT_MEASURED.to_owned()),
            ("Gateway", "None".to_owned()),
            ("DNS", "None".to_owned()),
        ]
    );
}

/// **With no link there is one line, saying why**: no adapter, not known
/// on this system, or the files would not read; and nothing before the
/// first reading.
#[test]
fn with_no_link_one_line_says_why() {
    let one = |reading: &NetworkReading| said(reading);
    assert_eq!(
        one(&reading(LinkState::NoAdapter, measured())),
        [("Network", "No network adapter".to_owned())]
    );
    assert_eq!(
        one(&reading(LinkState::Unknown, measured())),
        [("Network", "Not known here".to_owned())]
    );
    let unread = NetworkReading {
        link: Some(Err("denied".to_owned())),
        graphs: measured(),
    };
    assert_eq!(one(&unread), [("Network", "Could not be read".to_owned())]);
    assert!(lines(&NetworkReading::default()).is_empty());
}

/// **The lines that fit are placed top to bottom; the graph goes under
/// them only where there is room** -- in a large widget, not a wide one.
#[test]
fn the_graph_goes_under_the_lines_where_there_is_room() {
    let large_laid = layout(5, large());
    assert_eq!(large_laid.lines.len(), 5);
    let graph = large_laid.graph.expect("no graph in a large widget");
    let last = large_laid.lines.last().unwrap().1;
    assert!(
        graph.bounds().y >= last.y + last.h,
        "the graph overlaps a line"
    );
    assert!(
        graph.bounds().y + graph.bounds().h <= large().y + large().h + 0.01,
        "the graph is below the content"
    );
    assert_eq!(graph.graph, Graph::Network);
    let wide_laid = layout(5, wide());
    assert!(wide_laid.graph.is_none(), "a graph squeezed into one cell");
    assert!(
        wide_laid.lines.len() >= 3,
        "{} lines in one cell",
        wide_laid.lines.len()
    );
    for (_, bounds) in &wide_laid.lines {
        assert!(bounds.y + bounds.h <= wide().y + wide().h, "a line below");
    }
    assert!(layout(1, large()).graph.is_none(), "a graph with no link");
}

/// **Each line is drawn in its row**: its label at the left, its value at
/// the right -- "Connected" in green, anything else the status says in the
/// quiet ink.
#[test]
fn each_line_is_drawn_in_its_row() {
    let p = Palette::for_mode(false);
    let shown = reading(LinkState::Link(qemu()), measured());
    let mut commands = Vec::new();
    render(&shown, large(), &p, 255, &mut commands);
    let texts: Vec<(&str, f32, Color)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, y, color, .. } => Some((text.as_str(), *y, *color)),
            _ => None,
        })
        .collect();
    let laid = layout(5, large());
    for (index, line) in lines(&shown).iter().enumerate() {
        let (_, bounds) = laid.lines[index];
        for word in [line.label, line.value.as_str()] {
            let (_, y, _) = texts
                .iter()
                .find(|(t, ..)| *t == word)
                .unwrap_or_else(|| panic!("{word:?} is not drawn"));
            assert_eq!(*y, bounds.y, "{word:?} off its row");
        }
    }
    let connected = texts.iter().find(|(t, ..)| *t == "Connected").unwrap();
    assert_eq!(connected.2, p.ink(p.green));
    let mut commands = Vec::new();
    render(
        &reading(LinkState::NoAdapter, Graphs::default()),
        large(),
        &p,
        255,
        &mut commands,
    );
    let quiet = commands.iter().find_map(|c| match c {
        RenderCommand::Text { text, color, .. } if text == "No network adapter" => Some(*color),
        _ => None,
    });
    assert_eq!(quiet, Some(p.subtext0));
}

/// **Everything it draws is its palette's**, in both modes, at any veil --
/// the graph's line included.
#[test]
fn every_colour_it_draws_comes_from_its_palette() {
    for light in [false, true] {
        let mut p = Palette::for_mode(light);
        p.accent = appearance::PINK;
        for shown in [
            reading(LinkState::Link(qemu()), measured()),
            reading(LinkState::NoAdapter, Graphs::default()),
        ] {
            for content in [large(), wide()] {
                let mut commands = Vec::new();
                render(&shown, content, &p, ODD_ALPHA, &mut commands);
                assert_drawn_from(&p, &commands, &[], &format!("the network (light={light})"));
            }
        }
    }
}

/// **Everything it draws is under the widget's veil**: on a translucent
/// widget its words and its graph are as translucent as its background.
#[test]
fn everything_it_draws_is_under_the_widgets_veil() {
    let p = Palette::for_mode(false);
    let mut commands = Vec::new();
    render(
        &reading(LinkState::Link(qemu()), measured()),
        large(),
        &p,
        ODD_ALPHA,
        &mut commands,
    );
    assert!(!commands.is_empty());
    for command in &commands {
        let color = match command {
            RenderCommand::Text { color, .. }
            | RenderCommand::FillRect { color, .. }
            | RenderCommand::Line { color, .. } => *color,
            _ => continue,
        };
        assert_eq!(color.a, ODD_ALPHA, "{command:?}");
    }
}

/// **The shell reads the link no oftener than its spacing**, from where it
/// was told to.
#[test]
fn the_link_is_read_no_oftener_than_the_spacing() {
    let dir = scratchdir::ScratchDir::new("network-sampler");
    let mut sampler = LinkSampler::of_dir(dir.path("net"));
    assert_eq!(sampler.link(), None);
    assert!(sampler.sample(1_000));
    assert_eq!(sampler.link(), Some(&Ok(LinkState::NoAdapter)));
    assert!(!sampler.sample(1_000 + LINK_SPACING_MS - 1));
    assert!(sampler.sample(1_000 + LINK_SPACING_MS));
}
