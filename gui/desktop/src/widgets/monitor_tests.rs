//! Tests for a system monitor's graphs: the counters' arithmetic, the minute
//! the sampler keeps, what each graph says, where the graphs go, and how they
//! are drawn.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use appearance::palette_check::assert_drawn_from;
use guitk::bytes::si_rate;

/// Accents that are none of the graphs' hues -- blue, green, teal, mauve --
/// nor the meters' peach, so "no graph follows the accent" cannot hold by
/// coincidence in the one run where a real failure would be hardest to see.
const SAFE_ACCENTS: [Color; 3] = [appearance::SAPPHIRE, appearance::PINK, appearance::YELLOW];

/// A veil no default gives a widget, so no wash is right by accident.
const ODD_ALPHA: u8 = 137;

/// A disk's line, `name` having spent `io_ms` doing I/O.
fn disk(name: &str, io_ms: u64) -> DiskStats {
    DiskStats {
        name: name.as_bytes().to_vec(),
        major: Some(8),
        minor: Some(0),
        reads_completed: Some(0),
        reads_merged: Some(0),
        sectors_read: Some(0),
        ms_reading: Some(0),
        writes_completed: Some(0),
        writes_merged: Some(0),
        sectors_written: Some(0),
        ms_writing: Some(0),
        ios_in_progress: Some(0),
        ms_doing_io: Some(io_ms),
        weighted_ms: Some(0),
    }
}

/// An interface's line, `name` having received `rx` bytes and sent `tx`.
fn iface(name: &str, rx: u64, tx: u64) -> NetDevice {
    NetDevice {
        name: name.as_bytes().to_vec(),
        rx_bytes: Some(rx),
        rx_packets: Some(0),
        tx_bytes: Some(tx),
        tx_packets: Some(0),
    }
}

/// **Disk activity is the busiest disk's share of the interval**: each disk's
/// time doing I/O differenced over the time between the reads -- the most of
/// them, not their average, and never more than all of the time.
#[test]
fn disk_activity_is_the_busiest_disks_share_of_the_interval() {
    let before = [disk("sda", 1000), disk("sda1", 900), disk("sdb", 500)];
    let after = [disk("sda", 1250), disk("sda1", 1100), disk("sdb", 1000)];
    assert_eq!(
        disk_busy_fraction(&before, &after, 1000),
        Some(0.5),
        "sdb's half"
    );
    assert_eq!(
        disk_busy_fraction(&[disk("sda", 0)], &[disk("sda", 3000)], 1000),
        Some(1.0),
        "a counter that ran ahead of the clock is all of the time, no more"
    );
}

/// **A disk one read lacks, or whose counter went back, says nothing**; with
/// none left, or no time between the reads, there is no reading.
#[test]
fn a_disk_one_read_lacks_or_that_went_back_is_left_out() {
    let before = [disk("sda", 1000), disk("sdb", 800)];
    let after = [disk("sda", 400), disk("sdb", 900), disk("sdc", 5000)];
    assert_eq!(
        disk_busy_fraction(&before, &after, 1000),
        Some(0.1),
        "sda went back and sdc is new: sdb's tenth alone"
    );
    assert_eq!(
        disk_busy_fraction(&[disk("sda", 1000)], &[disk("sda", 400)], 1000),
        None
    );
    assert_eq!(disk_busy_fraction(&[], &[], 1000), None, "no disks");
    assert_eq!(
        disk_busy_fraction(&before, &[disk("sdb", 900)], 0),
        None,
        "no time"
    );
    let mut unknown = disk("sdb", 900);
    unknown.ms_doing_io = None;
    assert_eq!(disk_busy_fraction(&before, &[unknown], 1000), None);
}

/// **The network's rate is every interface's but the loopback's**, in and
/// out apart, over the time between the reads.
#[test]
fn the_network_rate_is_every_interfaces_but_the_loopbacks() {
    let before = [
        iface("lo", 0, 0),
        iface("eth0", 10_000, 5_000),
        iface("wlan0", 0, 700),
    ];
    let after = [
        iface("lo", 900_000, 900_000),
        iface("eth0", 11_000, 5_500),
        iface("wlan0", 3_000, 700),
    ];
    assert_eq!(
        network_rate(&before, &after, 500),
        Some(NetRate {
            down: 8_000,
            up: 1_000
        }),
        "4000 bytes in and 500 out over half a second"
    );
    assert_eq!(
        NetRate {
            down: 8_000,
            up: 1_000
        }
        .total(),
        9_000
    );
}

/// **An interface one read lacks, or whose counters went back, says
/// nothing**; with none left -- the loopback alone -- or no time, there is
/// no rate.
#[test]
fn an_interface_one_read_lacks_or_that_went_back_is_left_out() {
    let before = [iface("eth0", 10_000, 5_000), iface("eth1", 100, 100)];
    let after = [
        iface("eth0", 2_000, 5_000),
        iface("eth1", 1_100, 300),
        iface("eth2", 9_999, 9_999),
    ];
    assert_eq!(
        network_rate(&before, &after, 1000),
        Some(NetRate {
            down: 1_000,
            up: 200
        }),
        "eth0 was brought up again and eth2 is new: eth1's alone"
    );
    assert_eq!(
        network_rate(
            &[iface("eth0", 10_000, 5_000)],
            &[iface("eth0", 2_000, 5_000)],
            1000
        ),
        None,
        "an interface brought up again, alone, is no rate -- not a rate of nothing"
    );
    assert_eq!(
        network_rate(&[iface("lo", 0, 0)], &[iface("lo", 5, 5)], 1000),
        None,
        "the loopback alone is no network"
    );
    assert_eq!(network_rate(&before, &after, 0), None, "no time");
    let mut half = iface("eth1", 1_100, 300);
    half.tx_bytes = None;
    assert_eq!(network_rate(&before, &[half], 1000), None);
}

/// **A sample is due at first, and then no sooner than the spacing after
/// the last.**
#[test]
fn a_sample_is_due_no_sooner_than_the_spacing() {
    let mut sampler = Sampler::new();
    assert!(sampler.due(0));
    sampler.record(1_000, Some(0.2), Some(0.4), None, None);
    assert!(!sampler.due(1_000 + MIN_SAMPLE_SPACING_MS - 1));
    assert!(sampler.due(1_000 + MIN_SAMPLE_SPACING_MS));
    assert!(!sampler.due(0), "a clock behind the last sample");
}

/// **Each graph keeps a minute, and a reading nothing measured empties its
/// graph** rather than leaving the line where it stopped.
#[test]
fn a_graph_keeps_a_minute_and_an_unmeasured_reading_empties_it() {
    let mut sampler = Sampler::new();
    for second in 0..70_u16 {
        let share = f32::from(second) / 100.0;
        sampler.record(u64::from(second) * 1000, Some(share), Some(0.5), None, None);
    }
    let graphs = sampler.graphs();
    assert_eq!(graphs.cpu.len(), HISTORY_SAMPLES);
    assert_eq!(graphs.cpu.first(), Some(&0.1), "the oldest ten went");
    assert_eq!(graphs.cpu.last(), Some(&0.69));
    assert!(graphs.disk_activity.is_empty() && graphs.network.is_empty());

    sampler.record(70_000, None, Some(0.5), None, None);
    let graphs = sampler.graphs();
    assert!(graphs.cpu.is_empty(), "the processor went unmeasured");
    assert_eq!(graphs.memory.len(), HISTORY_SAMPLES);
}

/// **The counters' first read measures nothing** -- a rate is a difference
/// -- **and the next one does**, over the time between them.
#[test]
fn the_counters_first_read_measures_nothing_and_the_next_does() {
    let mut sampler = Sampler::new();
    sampler.record(
        0,
        Some(0.1),
        Some(0.1),
        Some(vec![disk("sda", 0)]),
        Some(vec![iface("eth0", 0, 0)]),
    );
    let first = sampler.graphs();
    assert!(first.disk_activity.is_empty() && first.network.is_empty());
    assert_eq!(first.network_now, None);

    sampler.record(
        2_000,
        Some(0.1),
        Some(0.1),
        Some(vec![disk("sda", 500)]),
        Some(vec![iface("eth0", 4_000, 2_000)]),
    );
    let next = sampler.graphs();
    assert_eq!(next.disk_activity, [0.25]);
    assert_eq!(
        next.network_now,
        Some(NetRate {
            down: 2_000,
            up: 1_000
        })
    );
    assert_eq!(next.network, [3_000.0]);

    sampler.record(3_000, Some(0.1), Some(0.1), None, None);
    let gone = sampler.graphs();
    assert!(gone.disk_activity.is_empty() && gone.network.is_empty());
    assert_eq!(gone.network_now, None, "nothing read, nothing to say");
}

/// **A graph's heading says what it reads now and at most**, and tools are
/// told the same in words -- the network's in and out apart; a graph nothing
/// measured says so.
#[test]
fn a_graph_says_what_it_reads_now_and_at_most() {
    let graphs = Graphs {
        cpu: vec![0.1, 0.8, 0.23],
        memory: vec![0.5],
        disk_activity: Vec::new(),
        network: vec![1_000_000.0, 3_400_000.0, 1_200_000.0],
        network_now: Some(NetRate {
            down: 1_000_000,
            up: 200_000,
        }),
    };
    assert_eq!(graphs.reading(Graph::Cpu).as_deref(), Some("23%, peak 80%"));
    assert_eq!(
        graphs.description(Graph::Cpu),
        "23% now, at most 80% in the last minute"
    );
    assert_eq!(
        graphs.reading(Graph::Memory).as_deref(),
        Some("50%, peak 50%")
    );
    assert_eq!(graphs.reading(Graph::DiskActivity), None);
    assert_eq!(graphs.description(Graph::DiskActivity), NOT_MEASURED);
    assert_eq!(
        graphs.reading(Graph::Network),
        Some(format!(
            "{}, peak {}",
            si_rate(1_200_000),
            si_rate(3_400_000)
        ))
    );
    assert_eq!(
        graphs.description(Graph::Network),
        format!(
            "{} in, {} out now, at most {} in the last minute",
            si_rate(1_000_000),
            si_rate(200_000),
            si_rate(3_400_000)
        )
    );
    assert_eq!(graphs.peak(Graph::Network), Some(3_400_000.0));
}

/// **The graphs share the content above the foot, top to bottom, apart**,
/// each a heading with its plot below it.
#[test]
fn the_graphs_share_the_content_above_the_foot() {
    let content = Rect::new(10.0, 20.0, 356.0, 280.0);
    let foot = 22.0;
    let placed = panels(content, foot);
    assert_eq!(
        placed.iter().map(|p| p.graph).collect::<Vec<_>>(),
        Graph::ALL
    );
    for panel in &placed {
        let plot = panel.plot.expect("room for a plot");
        assert!(
            plot.y >= panel.heading.bottom(),
            "{:?}'s plot under its heading",
            panel.graph
        );
        let bounds = panel.bounds();
        assert_eq!(content.union(bounds), content, "{:?} inside", panel.graph);
        assert!(
            bounds.bottom() <= content.bottom() - foot + 0.01,
            "{:?} clear of the foot",
            panel.graph
        );
    }
    for pair in placed.windows(2) {
        assert!(
            pair[0].bounds().bottom() < pair[1].bounds().y,
            "{:?} above {:?}, apart",
            pair[0].graph,
            pair[1].graph
        );
    }
}

/// **A monitor too short for a plot keeps its headings**, apart, and plots
/// nothing.
#[test]
fn a_short_monitor_drops_its_plots_and_keeps_its_headings() {
    let heading = heading_height();
    let content = Rect::new(0.0, 0.0, 300.0, 4.0 * (heading + PANEL_GAP) + 22.0);
    let placed = panels(content, 22.0);
    assert!(placed.iter().all(|p| p.plot.is_none()));
    for pair in placed.windows(2) {
        assert!(pair[0].heading.bottom() <= pair[1].heading.y);
    }
}

/// The graphs drawn from `graphs` in a roomy monitor.
fn drawn(graphs: &Graphs, p: &Palette) -> (Vec<Panel>, Vec<RenderCommand>) {
    drawn_at(graphs, p, 255)
}

/// The graphs drawn from `graphs` in a roomy monitor, under a veil of
/// `alpha`.
fn drawn_at(graphs: &Graphs, p: &Palette, alpha: u8) -> (Vec<Panel>, Vec<RenderCommand>) {
    let placed = panels(Rect::new(0.0, 0.0, 356.0, 280.0), 22.0);
    let mut commands = Vec::new();
    render(&placed, graphs, p, alpha, &mut commands);
    (placed, commands)
}

/// Every graph measured, so every branch is drawn: lines, guides, and
/// headings with readings.
fn every_graph() -> Graphs {
    Graphs {
        cpu: vec![0.2, 0.6],
        memory: vec![0.5, 0.4],
        disk_activity: vec![0.1, 0.9],
        network: vec![1_000.0, 4_000.0],
        network_now: Some(NetRate {
            down: 3_000,
            up: 1_000,
        }),
    }
}

/// The colour each graph's own role is.
fn role_of(graph: Graph, p: &Palette) -> Color {
    match graph {
        Graph::Cpu => p.blue,
        Graph::Memory => p.green,
        Graph::DiskActivity => p.teal,
        Graph::Network => p.mauve,
    }
}

/// Every colour a monitor's graphs draw is its palette's, in both modes,
/// whatever the accent, measured or not.
#[test]
fn every_colour_the_graphs_draw_comes_from_the_palette() {
    for light in [false, true] {
        for accent in SAFE_ACCENTS {
            let mut p = Palette::for_mode(light);
            p.accent = accent;
            for graphs in [every_graph(), Graphs::default()] {
                let (_, commands) = drawn(&graphs, &p);
                assert_drawn_from(
                    &p,
                    &commands,
                    &[],
                    &format!("the monitor's graphs (light={light})"),
                );
            }
        }
    }
}

/// **Each graph's line is drawn in the role it names, in its own panel, and
/// none in the accent** -- a hue names a measurement, and the accent marks
/// what the user chose. Rotated hues are still four distinct colours, so
/// each line is checked where it lies.
#[test]
fn each_graph_is_its_own_role_and_none_follows_the_accent() {
    for light in [false, true] {
        for accent in SAFE_ACCENTS {
            let mut p = Palette::for_mode(light);
            p.accent = accent;
            let (placed, commands) = drawn(&every_graph(), &p);
            let series: Vec<(f32, Color)> = commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Line {
                        y1,
                        y2,
                        color,
                        width,
                        ..
                    } if *width == LINE_WIDTH => Some(((y1 + y2) / 2.0, *color)),
                    _ => None,
                })
                .collect();
            assert_eq!(series.len(), 4, "one segment each");
            for (middle, color) in series {
                let panel = placed
                    .iter()
                    .find(|panel| {
                        panel
                            .plot
                            .is_some_and(|plot| middle >= plot.y && middle <= plot.bottom())
                    })
                    .expect("in a plot");
                assert_eq!(
                    color,
                    role_of(panel.graph, &p),
                    "{:?} (light={light})",
                    panel.graph
                );
                assert_ne!(color, p.accent, "{:?} follows the accent", panel.graph);
            }
        }
    }
}

/// **The guide lines are furniture**: the surface ramp's, and none of the
/// hues a reading is drawn in -- a guide in a reading's colour is a
/// reading nobody took.
#[test]
fn the_guide_lines_are_furniture() {
    for light in [false, true] {
        let p = Palette::for_mode(light);
        let (_, commands) = drawn(&every_graph(), &p);
        let guides: Vec<Color> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Line { color, width, .. } if *width == GRID_WIDTH => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(guides.len(), 4 * GRID_AT.len());
        for guide in guides {
            assert_eq!(guide, p.surface1, "light={light}");
            for graph in Graph::ALL {
                assert_ne!(guide, role_of(graph, &p), "a guide in {graph:?}'s hue");
            }
        }
    }
}

/// The texts drawn, with their colours: `(text, colour)`.
fn inks(commands: &[RenderCommand]) -> Vec<(String, Color)> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect()
}

/// **A heading names its graph in the secondary ink and says its reading in
/// the primary** -- the reading is the news -- and a graph nothing measures
/// says so in the secondary.
#[test]
fn a_heading_names_its_graph_dim_and_its_reading_bright() {
    for light in [false, true] {
        let p = Palette::for_mode(light);
        let graphs = Graphs {
            cpu: vec![0.5],
            ..Graphs::default()
        };
        let (_, commands) = drawn(&graphs, &p);
        let said = inks(&commands);
        assert_eq!(said[0], ("CPU".to_owned(), p.subtext0));
        assert_eq!(said[1], ("50%, peak 50%".to_owned(), p.text));
        assert_eq!(said[2], ("Memory".to_owned(), p.subtext0));
        assert_eq!(said[3], (NOT_MEASURED.to_owned(), p.subtext0));
        assert_ne!(p.text, p.subtext0);
    }
}

/// **The graphs are drawn under the widget's veil**, every line and every
/// word of them, as the rest of the widget is.
#[test]
fn the_graphs_are_drawn_under_the_widgets_veil() {
    let p = Palette::for_mode(false);
    let (_, commands) = drawn_at(&every_graph(), &p, ODD_ALPHA);
    let alphas: Vec<u8> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Line { color, .. } | RenderCommand::Text { color, .. } => Some(color.a),
            _ => None,
        })
        .collect();
    assert_eq!(alphas.len(), 4 * 2 + 4 * GRID_AT.len() + 4);
    assert!(alphas.iter().all(|&a| a == ODD_ALPHA), "{alphas:?}");
}

/// The lines drawn in `color`: `(x1, y1, x2, y2)`.
fn lines_in(commands: &[RenderCommand], color: Color) -> Vec<(f32, f32, f32, f32)> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: ink,
                ..
            } if *ink == color => Some((*x1, *y1, *x2, *y2)),
            _ => None,
        })
        .collect()
}

/// **Each graph is drawn in its own hue over its guide lines**, the newest
/// sample at the plot's right edge, a share on its whole: all of it at the
/// top, none at the floor.
#[test]
fn each_graph_is_drawn_in_its_hue_newest_at_the_right() {
    let p = Palette::for_mode(false);
    let graphs = Graphs {
        cpu: vec![0.0, 0.5, 1.0],
        memory: vec![0.25, 0.25],
        disk_activity: Vec::new(),
        network: Vec::new(),
        network_now: None,
    };
    let (placed, commands) = drawn(&graphs, &p);
    let cpu = lines_in(&commands, p.blue);
    assert_eq!(cpu.len(), 2, "three samples, two segments");
    let plot = placed[0].plot.unwrap();
    let last = cpu[1];
    assert_eq!(last.2, plot.right(), "the newest at the right edge");
    assert!((last.3 - plot.y).abs() < 0.01, "all of it at the top");
    assert!(
        (cpu[0].1 - plot.bottom()).abs() < 0.01,
        "none of it at the floor"
    );
    let step = plot.w / 59.0;
    assert!(
        (cpu[0].0 - (plot.right() - 2.0 * step)).abs() < 0.01,
        "a minute across: the oldest of three two steps back"
    );
    let memory = lines_in(&commands, p.green);
    assert_eq!(memory.len(), 1);
    let plot = placed[1].plot.unwrap();
    assert!(
        (memory[0].1 - (plot.bottom() - plot.h / 4.0)).abs() < 0.01,
        "a quarter of the whole a quarter of the way up -- a share is plotted on its whole, not on its minute's peak"
    );
    assert!(lines_in(&commands, p.teal).is_empty());
    assert!(lines_in(&commands, p.mauve).is_empty());
    assert_eq!(
        lines_in(&commands, p.surface1).len(),
        4 * GRID_AT.len(),
        "each plot's guide lines, measured or not"
    );
}

/// **The network is plotted on the most it carried in its minute**: its
/// busiest second at the top.
#[test]
fn the_network_is_plotted_on_its_minutes_peak() {
    let p = Palette::for_mode(true);
    let graphs = Graphs {
        network: vec![1_000.0, 2_000.0],
        network_now: Some(NetRate { down: 2_000, up: 0 }),
        ..Graphs::default()
    };
    let (placed, commands) = drawn(&graphs, &p);
    let plot = placed[3].plot.unwrap();
    let net = lines_in(&commands, p.mauve);
    assert_eq!(net.len(), 1);
    assert!((net[0].3 - plot.y).abs() < 0.01, "the peak at the top");
    assert!(
        (net[0].1 - (plot.y + plot.h / 2.0)).abs() < 0.01,
        "half the peak halfway"
    );
    let quiet = Graphs {
        network: vec![0.0, 0.0],
        network_now: Some(NetRate::default()),
        ..Graphs::default()
    };
    let (_, commands) = drawn(&quiet, &p);
    let net = lines_in(&commands, p.mauve);
    assert_eq!(net.len(), 1);
    assert!(
        (net[0].1 - plot.bottom()).abs() < 0.01,
        "a quiet minute along the floor"
    );
}

/// The texts drawn, with where: `(x, text)`.
fn texts(commands: &[RenderCommand]) -> Vec<(f32, String)> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { x, text, .. } => Some((*x, text.clone())),
            _ => None,
        })
        .collect()
}

/// **Each heading says its graph and what it reads, or that nothing
/// measures it** -- the reading against the right edge, never back over the
/// label.
#[test]
fn each_heading_says_its_graph_and_reading() {
    let p = Palette::for_mode(false);
    let graphs = Graphs {
        cpu: vec![0.23],
        ..Graphs::default()
    };
    let (placed, commands) = drawn(&graphs, &p);
    let said = texts(&commands);
    let words: Vec<&str> = said.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        words,
        [
            "CPU",
            "23%, peak 23%",
            "Memory",
            NOT_MEASURED,
            "Disk activity",
            NOT_MEASURED,
            "Network",
            NOT_MEASURED
        ]
    );
    let reading = &said[1];
    let width = guitk::text::measure(&reading.1, HEADING_SIZE, FontWeightHint::Regular);
    assert!(
        (reading.0 + width - placed[0].heading.right()).abs() < 0.01,
        "against the right edge"
    );

    let narrow = vec![Panel {
        graph: Graph::DiskActivity,
        heading: Rect::new(0.0, 0.0, 40.0, 14.0),
        plot: None,
    }];
    let mut commands = Vec::new();
    render(&narrow, &graphs, &p, 255, &mut commands);
    let said = texts(&commands);
    let label_end = guitk::text::measure("Disk activity", HEADING_SIZE, FontWeightHint::Bold);
    assert!(said[1].0 >= label_end, "not back over the label");
}
