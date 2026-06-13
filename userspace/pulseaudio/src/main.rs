//! Multi-personality PulseAudio sound system for SlateOS.
//!
//! This binary detects the personality from `argv[0]`:
//!   - `pactl`       -> PulseAudio control tool (default)
//!   - `pacmd`       -> PulseAudio command-line daemon interface
//!   - `paplay`      -> play audio files
//!   - `parecord`    -> record audio
//!   - `pasuspender` -> suspend PulseAudio temporarily
//!   - `pulseaudio`  -> PulseAudio daemon

#![deny(clippy::all)]

use std::env;
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION: &str = "0.1.0";

/// Maximum number of sinks the simulated server tracks.
/// Reserved for future server-side resource limits; validated in tests.
#[allow(dead_code)]
const MAX_SINKS: usize = 8;

/// Maximum number of sources the simulated server tracks.
#[allow(dead_code)]
const MAX_SOURCES: usize = 8;

/// Maximum number of modules the simulated server tracks.
const MAX_MODULES: usize = 64;

/// Maximum number of cards the simulated server tracks.
#[allow(dead_code)]
const MAX_CARDS: usize = 8;

/// Maximum number of active sink inputs.
#[allow(dead_code)]
const MAX_SINK_INPUTS: usize = 32;

/// Maximum number of active source outputs.
#[allow(dead_code)]
const MAX_SOURCE_OUTPUTS: usize = 32;

/// Default sample rate for audio playback/recording.
const DEFAULT_SAMPLE_RATE: u32 = 44100;

/// Default number of channels.
const DEFAULT_CHANNELS: u16 = 2;

/// Default volume (0-65536 range, 65536 = 100%).
const DEFAULT_VOLUME: u32 = 65536;

/// Volume value representing 100%.
const VOLUME_NORM: u32 = 65536;

/// Volume value representing 0% (silence).
const _VOLUME_MUTED: u32 = 0;

/// Maximum volume (150%).
const VOLUME_MAX: u32 = 98304;

// ---------------------------------------------------------------------------
// Sample format
// ---------------------------------------------------------------------------

/// Audio sample formats supported by the simulated PulseAudio server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleFormat {
    U8,
    _Alaw,
    _Ulaw,
    S16Le,
    _S16Be,
    Float32Le,
    _Float32Be,
    S32Le,
    _S32Be,
    S24Le,
    _S24Be,
    _S2432Le,
    _S2432Be,
}

impl SampleFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::_Alaw => "aLaw",
            Self::_Ulaw => "uLaw",
            Self::S16Le => "s16le",
            Self::_S16Be => "s16be",
            Self::Float32Le => "float32le",
            Self::_Float32Be => "float32be",
            Self::S32Le => "s32le",
            Self::_S32Be => "s32be",
            Self::S24Le => "s24le",
            Self::_S24Be => "s24be",
            Self::_S2432Le => "s24-32le",
            Self::_S2432Be => "s24-32be",
        }
    }

    fn bytes_per_sample(self) -> u32 {
        match self {
            Self::U8 | Self::_Alaw | Self::_Ulaw => 1,
            Self::S16Le | Self::_S16Be => 2,
            Self::S24Le | Self::_S24Be => 3,
            Self::Float32Le
            | Self::_Float32Be
            | Self::S32Le
            | Self::_S32Be
            | Self::_S2432Le
            | Self::_S2432Be => 4,
        }
    }

    fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "u8" => Some(Self::U8),
            "aLaw" | "alaw" => Some(Self::_Alaw),
            "uLaw" | "ulaw" => Some(Self::_Ulaw),
            "s16le" => Some(Self::S16Le),
            "s16be" => Some(Self::_S16Be),
            "float32le" => Some(Self::Float32Le),
            "float32be" => Some(Self::_Float32Be),
            "s32le" => Some(Self::S32Le),
            "s32be" => Some(Self::_S32Be),
            "s24le" => Some(Self::S24Le),
            "s24be" => Some(Self::_S24Be),
            "s24-32le" => Some(Self::_S2432Le),
            "s24-32be" => Some(Self::_S2432Be),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Sample spec
// ---------------------------------------------------------------------------

/// Describes the format, rate, and channel count for audio data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SampleSpec {
    format: SampleFormat,
    rate: u32,
    channels: u16,
}

impl SampleSpec {
    fn new(format: SampleFormat, rate: u32, channels: u16) -> Self {
        Self {
            format,
            rate,
            channels,
        }
    }

    fn bytes_per_second(self) -> u64 {
        u64::from(self.format.bytes_per_sample()) * u64::from(self.rate) * u64::from(self.channels)
    }

    // Reserved for buffer-size math once the playback/record paths are wired;
    // currently exercised only by unit tests.
    #[allow(dead_code)]
    fn frame_size(self) -> u32 {
        self.format.bytes_per_sample() * u32::from(self.channels)
    }
}

impl std::fmt::Display for SampleSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}ch {}Hz",
            self.format.as_str(),
            self.channels,
            self.rate
        )
    }
}

// ---------------------------------------------------------------------------
// Channel map
// ---------------------------------------------------------------------------

/// Identifies a single audio channel position.
// Some positions (rear-center, side-left/right) are defined for completeness of
// the channel-map model but are not yet produced by any built-in map preset.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelPosition {
    Mono,
    FrontLeft,
    FrontRight,
    FrontCenter,
    RearLeft,
    RearRight,
    RearCenter,
    Lfe,
    SideLeft,
    SideRight,
}

impl ChannelPosition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mono => "mono",
            Self::FrontLeft => "front-left",
            Self::FrontRight => "front-right",
            Self::FrontCenter => "front-center",
            Self::RearLeft => "rear-left",
            Self::RearRight => "rear-right",
            Self::RearCenter => "rear-center",
            Self::Lfe => "lfe",
            Self::SideLeft => "side-left",
            Self::SideRight => "side-right",
        }
    }
}

/// Maps channel indices to positions.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ChannelMap {
    positions: Vec<ChannelPosition>,
}

impl ChannelMap {
    fn stereo() -> Self {
        Self {
            positions: vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
        }
    }

    fn mono() -> Self {
        Self {
            positions: vec![ChannelPosition::Mono],
        }
    }

    // 5.1 channel-map preset; reserved for multichannel device setup, tested.
    #[allow(dead_code)]
    fn surround51() -> Self {
        Self {
            positions: vec![
                ChannelPosition::FrontLeft,
                ChannelPosition::FrontRight,
                ChannelPosition::FrontCenter,
                ChannelPosition::Lfe,
                ChannelPosition::RearLeft,
                ChannelPosition::RearRight,
            ],
        }
    }
}

impl std::fmt::Display for ChannelMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for p in &self.positions {
            if !first {
                write!(f, ",")?;
            }
            write!(f, "{}", p.as_str())?;
            first = false;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

/// Represents a volume level for one or more channels.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Volume {
    values: Vec<u32>,
}

impl Volume {
    fn new(channels: u16, level: u32) -> Self {
        Self {
            values: vec![level; channels as usize],
        }
    }

    fn percent_str(&self) -> String {
        self.values
            .iter()
            .map(|v| format!("{}%", (*v as u64 * 100) / u64::from(VOLUME_NORM)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn db_str(&self) -> String {
        self.values
            .iter()
            .map(|v| {
                if *v == 0 {
                    "-inf dB".to_string()
                } else {
                    let ratio = *v as f64 / f64::from(VOLUME_NORM);
                    let db = 20.0 * ratio.log10();
                    format!("{db:.2} dB")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn raw_str(&self) -> String {
        self.values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn average(&self) -> u32 {
        if self.values.is_empty() {
            return 0;
        }
        let sum: u64 = self.values.iter().copied().map(u64::from).sum();
        (sum / self.values.len() as u64) as u32
    }

    fn set_all(&mut self, level: u32) {
        for v in &mut self.values {
            *v = level;
        }
    }
}

impl std::fmt::Display for Volume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.raw_str())
    }
}

// ---------------------------------------------------------------------------
// Sink state
// ---------------------------------------------------------------------------

/// Operating state of a sink.
// Running/Suspended are part of the state model but the simulated server only
// ever reports Idle today; reserved for when real streams drive state changes.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SinkState {
    Running,
    Idle,
    Suspended,
}

impl SinkState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Idle => "IDLE",
            Self::Suspended => "SUSPENDED",
        }
    }
}

// ---------------------------------------------------------------------------
// Source state
// ---------------------------------------------------------------------------

/// Operating state of a source.
// See SinkState: Running/Suspended reserved for live-stream state transitions.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceState {
    Running,
    Idle,
    Suspended,
}

impl SourceState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Idle => "IDLE",
            Self::Suspended => "SUSPENDED",
        }
    }
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

/// Priority for a device port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortAvailability {
    Unknown,
    Available,
    _Unavailable,
}

impl PortAvailability {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::_Unavailable => "not available",
        }
    }
}

/// Represents a hardware port on a sink or source.
#[derive(Clone, Debug)]
struct Port {
    name: String,
    description: String,
    priority: u32,
    availability: PortAvailability,
}

impl Port {
    fn new(name: &str, desc: &str, priority: u32, availability: PortAvailability) -> Self {
        Self {
            name: name.to_string(),
            description: desc.to_string(),
            priority,
            availability,
        }
    }
}

// ---------------------------------------------------------------------------
// Sink
// ---------------------------------------------------------------------------

/// A PulseAudio output device (sink).
#[derive(Clone, Debug)]
struct Sink {
    index: u32,
    name: String,
    description: String,
    driver: String,
    sample_spec: SampleSpec,
    channel_map: ChannelMap,
    volume: Volume,
    muted: bool,
    state: SinkState,
    ports: Vec<Port>,
    active_port: Option<String>,
    _owner_module: u32,
    _monitor_source: u32,
    _latency_usec: u64,
    _configured_latency_usec: u64,
    _base_volume: u32,
}

impl Sink {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  * index: {}\n", self.index));
        out.push_str(&format!("\tname: <{}>\n", self.name));
        out.push_str(&format!("\tdriver: <{}>\n", self.driver));
        out.push_str(&format!("\tstate: {}\n", self.state.as_str()));
        out.push_str(&format!("\tsample spec: {}\n", self.sample_spec));
        out.push_str(&format!("\tchannel map: {}\n", self.channel_map));
        out.push_str(&format!(
            "\tvolume: {} / {} / {}\n",
            self.volume.raw_str(),
            self.volume.percent_str(),
            self.volume.db_str()
        ));
        out.push_str(&format!(
            "\tmuted: {}\n",
            if self.muted { "yes" } else { "no" }
        ));
        out.push_str(&format!("\tdescription: {}\n", self.description));
        out.push_str(&format!("\tmonitor source: {}\n", self._monitor_source));
        out.push_str(&format!("\tlatency: {} usec\n", self._latency_usec));
        out.push_str(&format!(
            "\tconfigured latency: {} usec\n",
            self._configured_latency_usec
        ));
        out.push_str(&format!("\tbase volume: {}\n", self._base_volume));
        out.push_str("\tports:\n");
        for p in &self.ports {
            out.push_str(&format!(
                "\t\t{}: {} (priority: {}, {})\n",
                p.name,
                p.description,
                p.priority,
                p.availability.as_str()
            ));
        }
        if let Some(ref ap) = self.active_port {
            out.push_str(&format!("\tactive port: <{ap}>\n"));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------

/// A PulseAudio input device (source).
#[derive(Clone, Debug)]
struct Source {
    index: u32,
    name: String,
    description: String,
    driver: String,
    sample_spec: SampleSpec,
    channel_map: ChannelMap,
    volume: Volume,
    muted: bool,
    state: SourceState,
    ports: Vec<Port>,
    active_port: Option<String>,
    _owner_module: u32,
    _monitor_of_sink: Option<u32>,
    _latency_usec: u64,
    _configured_latency_usec: u64,
    _base_volume: u32,
}

impl Source {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  * index: {}\n", self.index));
        out.push_str(&format!("\tname: <{}>\n", self.name));
        out.push_str(&format!("\tdriver: <{}>\n", self.driver));
        out.push_str(&format!("\tstate: {}\n", self.state.as_str()));
        out.push_str(&format!("\tsample spec: {}\n", self.sample_spec));
        out.push_str(&format!("\tchannel map: {}\n", self.channel_map));
        out.push_str(&format!(
            "\tvolume: {} / {} / {}\n",
            self.volume.raw_str(),
            self.volume.percent_str(),
            self.volume.db_str()
        ));
        out.push_str(&format!(
            "\tmuted: {}\n",
            if self.muted { "yes" } else { "no" }
        ));
        out.push_str(&format!("\tdescription: {}\n", self.description));
        if let Some(m) = self._monitor_of_sink {
            out.push_str(&format!("\tmonitor of sink: {m}\n"));
        }
        out.push_str(&format!("\tlatency: {} usec\n", self._latency_usec));
        out.push_str(&format!(
            "\tconfigured latency: {} usec\n",
            self._configured_latency_usec
        ));
        out.push_str(&format!("\tbase volume: {}\n", self._base_volume));
        out.push_str("\tports:\n");
        for p in &self.ports {
            out.push_str(&format!(
                "\t\t{}: {} (priority: {}, {})\n",
                p.name,
                p.description,
                p.priority,
                p.availability.as_str()
            ));
        }
        if let Some(ref ap) = self.active_port {
            out.push_str(&format!("\tactive port: <{ap}>\n"));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Sink input
// ---------------------------------------------------------------------------

/// A stream playing to a sink.
#[derive(Clone, Debug)]
struct SinkInput {
    index: u32,
    name: String,
    driver: String,
    sink: u32,
    sample_spec: SampleSpec,
    channel_map: ChannelMap,
    volume: Volume,
    muted: bool,
    _owner_module: Option<u32>,
    _client: Option<u32>,
    _buffer_latency_usec: u64,
    _sink_latency_usec: u64,
}

impl SinkInput {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  * index: {}\n", self.index));
        out.push_str(&format!("\tname: <{}>\n", self.name));
        out.push_str(&format!("\tdriver: <{}>\n", self.driver));
        out.push_str(&format!("\tsink: {} \n", self.sink));
        out.push_str(&format!("\tsample spec: {}\n", self.sample_spec));
        out.push_str(&format!("\tchannel map: {}\n", self.channel_map));
        out.push_str(&format!(
            "\tvolume: {} / {} / {}\n",
            self.volume.raw_str(),
            self.volume.percent_str(),
            self.volume.db_str()
        ));
        out.push_str(&format!(
            "\tmuted: {}\n",
            if self.muted { "yes" } else { "no" }
        ));
        if let Some(c) = self._client {
            out.push_str(&format!("\tclient: {c}\n"));
        }
        out.push_str(&format!(
            "\tbuffer latency: {} usec\n",
            self._buffer_latency_usec
        ));
        out.push_str(&format!(
            "\tsink latency: {} usec\n",
            self._sink_latency_usec
        ));
        out
    }
}

// ---------------------------------------------------------------------------
// Source output
// ---------------------------------------------------------------------------

/// A stream recording from a source.
#[derive(Clone, Debug)]
struct SourceOutput {
    index: u32,
    name: String,
    driver: String,
    source: u32,
    sample_spec: SampleSpec,
    channel_map: ChannelMap,
    volume: Volume,
    muted: bool,
    _owner_module: Option<u32>,
    _client: Option<u32>,
    _buffer_latency_usec: u64,
    _source_latency_usec: u64,
}

impl SourceOutput {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  * index: {}\n", self.index));
        out.push_str(&format!("\tname: <{}>\n", self.name));
        out.push_str(&format!("\tdriver: <{}>\n", self.driver));
        out.push_str(&format!("\tsource: {} \n", self.source));
        out.push_str(&format!("\tsample spec: {}\n", self.sample_spec));
        out.push_str(&format!("\tchannel map: {}\n", self.channel_map));
        out.push_str(&format!(
            "\tvolume: {} / {} / {}\n",
            self.volume.raw_str(),
            self.volume.percent_str(),
            self.volume.db_str()
        ));
        out.push_str(&format!(
            "\tmuted: {}\n",
            if self.muted { "yes" } else { "no" }
        ));
        if let Some(c) = self._client {
            out.push_str(&format!("\tclient: {c}\n"));
        }
        out.push_str(&format!(
            "\tbuffer latency: {} usec\n",
            self._buffer_latency_usec
        ));
        out.push_str(&format!(
            "\tsource latency: {} usec\n",
            self._source_latency_usec
        ));
        out
    }
}

// ---------------------------------------------------------------------------
// Card profile
// ---------------------------------------------------------------------------

/// A hardware profile for a sound card.
#[derive(Clone, Debug)]
struct CardProfile {
    name: String,
    description: String,
    _n_sinks: u32,
    _n_sources: u32,
    _priority: u32,
    _available: bool,
}

impl CardProfile {
    fn new(name: &str, desc: &str, sinks: u32, sources: u32, prio: u32) -> Self {
        Self {
            name: name.to_string(),
            description: desc.to_string(),
            _n_sinks: sinks,
            _n_sources: sources,
            _priority: prio,
            _available: true,
        }
    }

    fn format_info(&self) -> String {
        format!(
            "\t\t{}: {} (sinks: {}, sources: {}, priority: {}{})\n",
            self.name,
            self.description,
            self._n_sinks,
            self._n_sources,
            self._priority,
            if self._available { ", available" } else { "" }
        )
    }
}

// ---------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------

/// A sound card with profiles.
#[derive(Clone, Debug)]
struct Card {
    index: u32,
    name: String,
    driver: String,
    profiles: Vec<CardProfile>,
    active_profile: String,
    _owner_module: u32,
}

impl Card {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  * index: {}\n", self.index));
        out.push_str(&format!("\tname: <{}>\n", self.name));
        out.push_str(&format!("\tdriver: <{}>\n", self.driver));
        out.push_str(&format!("\towner module: {}\n", self._owner_module));
        out.push_str("\tprofiles:\n");
        for p in &self.profiles {
            out.push_str(&p.format_info());
        }
        out.push_str(&format!("\tactive profile: <{}>\n", self.active_profile));
        out
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

/// A loaded PulseAudio module.
#[derive(Clone, Debug)]
struct Module {
    index: u32,
    name: String,
    argument: String,
    _n_used: i32,
}

impl Module {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  * index: {}\n", self.index));
        out.push_str(&format!("\tname: <{}>\n", self.name));
        out.push_str(&format!("\targument: <{}>\n", self.argument));
        let used_str = if self._n_used < 0 {
            "n/a".to_string()
        } else {
            self._n_used.to_string()
        };
        out.push_str(&format!("\tused: {used_str}\n"));
        out
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// A connected PulseAudio client.
#[derive(Clone, Debug)]
struct Client {
    index: u32,
    name: String,
    driver: String,
    _owner_module: Option<u32>,
}

impl Client {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  * index: {}\n", self.index));
        out.push_str(&format!("\tname: <{}>\n", self.name));
        out.push_str(&format!("\tdriver: <{}>\n", self.driver));
        if let Some(m) = self._owner_module {
            out.push_str(&format!("\towner module: {m}\n"));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Server info
// ---------------------------------------------------------------------------

/// Server-level metadata.
#[derive(Clone, Debug)]
struct ServerInfo {
    server_name: String,
    server_version: String,
    default_sink_name: String,
    default_source_name: String,
    default_sample_spec: SampleSpec,
    default_channel_map: ChannelMap,
    _host_name: String,
    _user_name: String,
    _cookie: u32,
}

impl ServerInfo {
    fn format_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Server String: {}\n", self.server_name));
        out.push_str(&format!(
            "Library Protocol Version: {}\n",
            self.server_version
        ));
        out.push_str(&format!("Default Sink: {}\n", self.default_sink_name));
        out.push_str(&format!("Default Source: {}\n", self.default_source_name));
        out.push_str(&format!(
            "Default Sample Specification: {}\n",
            self.default_sample_spec
        ));
        out.push_str(&format!(
            "Default Channel Map: {}\n",
            self.default_channel_map
        ));
        out.push_str(&format!("Host Name: {}\n", self._host_name));
        out.push_str(&format!("User Name: {}\n", self._user_name));
        out.push_str(&format!("Cookie: {:08x}\n", self._cookie));
        out
    }
}

// ---------------------------------------------------------------------------
// Stat info
// ---------------------------------------------------------------------------

/// Memory/buffer statistics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StatInfo {
    memblock_total: u32,
    memblock_total_size: u64,
    memblock_allocated: u32,
    memblock_allocated_size: u64,
    scache_size: u64,
}

impl StatInfo {
    fn format_info(self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Currently in use: {} blocks containing {} bytes total.\n",
            self.memblock_allocated, self.memblock_allocated_size
        ));
        out.push_str(&format!(
            "Allocated during whole lifetime: {} blocks containing {} bytes total.\n",
            self.memblock_total, self.memblock_total_size
        ));
        out.push_str(&format!("Sample cache size: {} bytes.\n", self.scache_size));
        out
    }
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

/// The entire simulated PulseAudio server state.
#[derive(Clone, Debug)]
struct PulseState {
    server: ServerInfo,
    sinks: Vec<Sink>,
    sources: Vec<Source>,
    sink_inputs: Vec<SinkInput>,
    source_outputs: Vec<SourceOutput>,
    cards: Vec<Card>,
    modules: Vec<Module>,
    clients: Vec<Client>,
    stat: StatInfo,
    next_module_index: u32,
    _running: bool,
}

impl PulseState {
    fn default_state() -> Self {
        let default_spec =
            SampleSpec::new(SampleFormat::S16Le, DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS);
        let default_map = ChannelMap::stereo();

        let mut state = Self {
            server: ServerInfo {
                server_name: "pulseaudio".to_string(),
                server_version: VERSION.to_string(),
                default_sink_name: "alsa_output.pci-0000_00_1f.3.analog-stereo".to_string(),
                default_source_name: "alsa_input.pci-0000_00_1f.3.analog-stereo".to_string(),
                default_sample_spec: default_spec,
                default_channel_map: default_map.clone(),
                _host_name: "slateos".to_string(),
                _user_name: "user".to_string(),
                _cookie: 0xdeadbeef,
            },
            sinks: Vec::new(),
            sources: Vec::new(),
            sink_inputs: Vec::new(),
            source_outputs: Vec::new(),
            cards: Vec::new(),
            modules: Vec::new(),
            clients: Vec::new(),
            stat: StatInfo {
                memblock_total: 1024,
                memblock_total_size: 4_194_304,
                memblock_allocated: 64,
                memblock_allocated_size: 262_144,
                scache_size: 0,
            },
            next_module_index: 10,
            _running: true,
        };

        // Default sink
        state.sinks.push(Sink {
            index: 0,
            name: "alsa_output.pci-0000_00_1f.3.analog-stereo".to_string(),
            description: "Built-in Audio Analog Stereo".to_string(),
            driver: "module-alsa-card.c".to_string(),
            sample_spec: default_spec,
            channel_map: default_map.clone(),
            volume: Volume::new(DEFAULT_CHANNELS, DEFAULT_VOLUME),
            muted: false,
            state: SinkState::Idle,
            ports: vec![
                Port::new(
                    "analog-output-speaker",
                    "Speakers",
                    10000,
                    PortAvailability::Available,
                ),
                Port::new(
                    "analog-output-headphones",
                    "Headphones",
                    9900,
                    PortAvailability::Unknown,
                ),
            ],
            active_port: Some("analog-output-speaker".to_string()),
            _owner_module: 1,
            _monitor_source: 1,
            _latency_usec: 32000,
            _configured_latency_usec: 40000,
            _base_volume: DEFAULT_VOLUME,
        });

        // Second sink (HDMI)
        state.sinks.push(Sink {
            index: 1,
            name: "alsa_output.pci-0000_01_00.1.hdmi-stereo".to_string(),
            description: "HDMI Audio Output".to_string(),
            driver: "module-alsa-card.c".to_string(),
            sample_spec: default_spec,
            channel_map: default_map.clone(),
            volume: Volume::new(DEFAULT_CHANNELS, DEFAULT_VOLUME),
            muted: false,
            state: SinkState::Suspended,
            ports: vec![Port::new(
                "hdmi-output-0",
                "HDMI / DisplayPort",
                5900,
                PortAvailability::Available,
            )],
            active_port: Some("hdmi-output-0".to_string()),
            _owner_module: 2,
            _monitor_source: 2,
            _latency_usec: 0,
            _configured_latency_usec: 40000,
            _base_volume: DEFAULT_VOLUME,
        });

        // Default source
        state.sources.push(Source {
            index: 0,
            name: "alsa_input.pci-0000_00_1f.3.analog-stereo".to_string(),
            description: "Built-in Audio Analog Stereo".to_string(),
            driver: "module-alsa-card.c".to_string(),
            sample_spec: SampleSpec::new(SampleFormat::S16Le, DEFAULT_SAMPLE_RATE, 1),
            channel_map: ChannelMap::mono(),
            volume: Volume::new(1, DEFAULT_VOLUME),
            muted: false,
            state: SourceState::Idle,
            ports: vec![Port::new(
                "analog-input-internal-mic",
                "Internal Microphone",
                8900,
                PortAvailability::Available,
            )],
            active_port: Some("analog-input-internal-mic".to_string()),
            _owner_module: 1,
            _monitor_of_sink: None,
            _latency_usec: 16000,
            _configured_latency_usec: 40000,
            _base_volume: DEFAULT_VOLUME,
        });

        // Monitor source for sink 0
        state.sources.push(Source {
            index: 1,
            name: "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor".to_string(),
            description: "Monitor of Built-in Audio Analog Stereo".to_string(),
            driver: "module-alsa-card.c".to_string(),
            sample_spec: default_spec,
            channel_map: default_map.clone(),
            volume: Volume::new(DEFAULT_CHANNELS, DEFAULT_VOLUME),
            muted: false,
            state: SourceState::Idle,
            ports: Vec::new(),
            active_port: None,
            _owner_module: 1,
            _monitor_of_sink: Some(0),
            _latency_usec: 0,
            _configured_latency_usec: 0,
            _base_volume: DEFAULT_VOLUME,
        });

        // A card
        state.cards.push(Card {
            index: 0,
            name: "alsa_card.pci-0000_00_1f.3".to_string(),
            driver: "module-alsa-card.c".to_string(),
            profiles: vec![
                CardProfile::new(
                    "output:analog-stereo+input:analog-stereo",
                    "Analog Stereo Duplex",
                    1,
                    1,
                    6565,
                ),
                CardProfile::new("output:analog-stereo", "Analog Stereo Output", 1, 0, 6500),
                CardProfile::new("input:analog-stereo", "Analog Stereo Input", 0, 1, 6500),
                CardProfile::new("off", "Off", 0, 0, 0),
            ],
            active_profile: "output:analog-stereo+input:analog-stereo".to_string(),
            _owner_module: 1,
        });

        // Default modules
        let default_modules = [
            (0, "module-device-restore", ""),
            (1, "module-stream-restore", ""),
            (2, "module-card-restore", ""),
            (3, "module-augment-properties", ""),
            (4, "module-switch-on-port-available", ""),
            (5, "module-alsa-card", "device_id=\"0\""),
            (6, "module-alsa-card", "device_id=\"1\""),
            (7, "module-native-protocol-unix", ""),
            (8, "module-default-device-restore", ""),
            (9, "module-always-sink", ""),
        ];
        for (idx, name, arg) in default_modules {
            state.modules.push(Module {
                index: idx,
                name: name.to_string(),
                argument: arg.to_string(),
                _n_used: -1,
            });
        }

        // Default clients
        state.clients.push(Client {
            index: 0,
            name: "PulseAudio Control".to_string(),
            driver: "protocol-native.c".to_string(),
            _owner_module: Some(7),
        });

        // A sink input
        state.sink_inputs.push(SinkInput {
            index: 0,
            name: "Playback Stream".to_string(),
            driver: "protocol-native.c".to_string(),
            sink: 0,
            sample_spec: default_spec,
            channel_map: default_map,
            volume: Volume::new(DEFAULT_CHANNELS, DEFAULT_VOLUME),
            muted: false,
            _owner_module: None,
            _client: Some(0),
            _buffer_latency_usec: 16000,
            _sink_latency_usec: 32000,
        });

        state
    }

    fn find_sink_by_name_or_index(&self, id: &str) -> Option<usize> {
        if let Ok(idx) = id.parse::<u32>() {
            self.sinks.iter().position(|s| s.index == idx)
        } else {
            self.sinks.iter().position(|s| s.name == id)
        }
    }

    fn find_source_by_name_or_index(&self, id: &str) -> Option<usize> {
        if let Ok(idx) = id.parse::<u32>() {
            self.sources.iter().position(|s| s.index == idx)
        } else {
            self.sources.iter().position(|s| s.name == id)
        }
    }

    fn find_card_by_name_or_index(&self, id: &str) -> Option<usize> {
        if let Ok(idx) = id.parse::<u32>() {
            self.cards.iter().position(|c| c.index == idx)
        } else {
            self.cards.iter().position(|c| c.name == id)
        }
    }

    fn find_module_by_index(&self, idx: u32) -> Option<usize> {
        self.modules.iter().position(|m| m.index == idx)
    }

    fn find_sink_input_by_index(&self, idx: u32) -> Option<usize> {
        self.sink_inputs.iter().position(|si| si.index == idx)
    }

    fn find_source_output_by_index(&self, idx: u32) -> Option<usize> {
        self.source_outputs.iter().position(|so| so.index == idx)
    }
}

// ---------------------------------------------------------------------------
// Volume parsing helpers
// ---------------------------------------------------------------------------

/// Parse a volume value from a string. Supports:
///  - plain integer (raw PA volume)
///  - percentage like "50%" or "+5%" or "-10%"
///  - dB like "0dB" or "-6dB" or "+3dB"
fn parse_volume(s: &str, current: u32) -> Result<u32, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty volume string".to_string());
    }

    // Check for percentage
    if let Some(pct_str) = s.strip_suffix('%') {
        return parse_relative_or_absolute_pct(pct_str, current);
    }

    // Check for dB
    if let Some(db_str) = s.strip_suffix("dB").or_else(|| s.strip_suffix("db")) {
        return parse_db(db_str, current);
    }

    // Plain integer
    if let Some(rest) = s.strip_prefix('+') {
        let delta: u32 = rest.parse().map_err(|_| format!("invalid volume: {s}"))?;
        let result = current.saturating_add(delta);
        Ok(result.min(VOLUME_MAX))
    } else if let Some(rest) = s.strip_prefix('-') {
        let delta: u32 = rest.parse().map_err(|_| format!("invalid volume: {s}"))?;
        Ok(current.saturating_sub(delta))
    } else {
        let v: u32 = s.parse().map_err(|_| format!("invalid volume: {s}"))?;
        Ok(v.min(VOLUME_MAX))
    }
}

fn parse_relative_or_absolute_pct(s: &str, current: u32) -> Result<u32, String> {
    if let Some(rest) = s.strip_prefix('+') {
        let pct: u32 = rest
            .parse()
            .map_err(|_| format!("invalid percentage: {s}%"))?;
        let delta = (u64::from(VOLUME_NORM) * u64::from(pct) / 100) as u32;
        let result = current.saturating_add(delta);
        Ok(result.min(VOLUME_MAX))
    } else if let Some(rest) = s.strip_prefix('-') {
        let pct: u32 = rest
            .parse()
            .map_err(|_| format!("invalid percentage: {s}%"))?;
        let delta = (u64::from(VOLUME_NORM) * u64::from(pct) / 100) as u32;
        Ok(current.saturating_sub(delta))
    } else {
        let pct: u32 = s.parse().map_err(|_| format!("invalid percentage: {s}%"))?;
        let vol = (u64::from(VOLUME_NORM) * u64::from(pct) / 100) as u32;
        Ok(vol.min(VOLUME_MAX))
    }
}

fn parse_db(s: &str, current: u32) -> Result<u32, String> {
    // Simple dB->linear conversion: volume = VOLUME_NORM * 10^(dB/20)
    let is_relative = s.starts_with('+') || (s.starts_with('-') && current > 0);
    let db_val: f64 = s.parse().map_err(|_| format!("invalid dB value: {s}dB"))?;

    if is_relative && (s.starts_with('+') || s.starts_with('-')) {
        let current_db = if current == 0 {
            -100.0_f64
        } else {
            20.0 * (current as f64 / f64::from(VOLUME_NORM)).log10()
        };
        let target_db = current_db + db_val;
        let linear = 10.0_f64.powf(target_db / 20.0);
        let vol = (linear * f64::from(VOLUME_NORM)) as u32;
        Ok(vol.min(VOLUME_MAX))
    } else {
        let linear = 10.0_f64.powf(db_val / 20.0);
        let vol = (linear * f64::from(VOLUME_NORM)) as u32;
        Ok(vol.min(VOLUME_MAX))
    }
}

/// Parse a mute value: "1", "true", "yes" -> true; "0", "false", "no" -> false;
/// "toggle" -> flip current.
fn parse_mute(s: &str, current: bool) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        "toggle" => Ok(!current),
        _ => Err(format!("invalid mute value: {s}")),
    }
}

// ---------------------------------------------------------------------------
// Personality enum
// ---------------------------------------------------------------------------

/// Which personality this binary is running as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Pactl,
    Pacmd,
    Paplay,
    Parecord,
    Pasuspender,
    Pulseaudio,
}

impl Personality {
    fn from_name(name: &str) -> Self {
        match name {
            "pacmd" => Self::Pacmd,
            "paplay" => Self::Paplay,
            "parecord" => Self::Parecord,
            "pasuspender" => Self::Pasuspender,
            "pulseaudio" => Self::Pulseaudio,
            _ => Self::Pactl,
        }
    }

    // Inverse of from_name; reserved for diagnostics/usage output, tested.
    #[allow(dead_code)]
    fn name(self) -> &'static str {
        match self {
            Self::Pactl => "pactl",
            Self::Pacmd => "pacmd",
            Self::Paplay => "paplay",
            Self::Parecord => "parecord",
            Self::Pasuspender => "pasuspender",
            Self::Pulseaudio => "pulseaudio",
        }
    }
}

// ---------------------------------------------------------------------------
// pactl subcommands
// ---------------------------------------------------------------------------

/// Recognized pactl subcommands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PactlCommand {
    List,
    Info,
    Stat,
    SetSinkVolume,
    SetSourceVolume,
    SetSinkMute,
    SetSourceMute,
    SetDefaultSink,
    SetDefaultSource,
    MoveSinkInput,
    MoveSourceOutput,
    LoadModule,
    UnloadModule,
    SetCardProfile,
    Subscribe,
    Exit,
}

impl PactlCommand {
    fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "list" => Some(Self::List),
            "info" => Some(Self::Info),
            "stat" => Some(Self::Stat),
            "set-sink-volume" => Some(Self::SetSinkVolume),
            "set-source-volume" => Some(Self::SetSourceVolume),
            "set-sink-mute" => Some(Self::SetSinkMute),
            "set-source-mute" => Some(Self::SetSourceMute),
            "set-default-sink" => Some(Self::SetDefaultSink),
            "set-default-source" => Some(Self::SetDefaultSource),
            "move-sink-input" => Some(Self::MoveSinkInput),
            "move-source-output" => Some(Self::MoveSourceOutput),
            "load-module" => Some(Self::LoadModule),
            "unload-module" => Some(Self::UnloadModule),
            "set-card-profile" => Some(Self::SetCardProfile),
            "subscribe" => Some(Self::Subscribe),
            "exit" => Some(Self::Exit),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// List entity type filter
// ---------------------------------------------------------------------------

/// The entity type to list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListEntity {
    Sinks,
    Sources,
    SinkInputs,
    SourceOutputs,
    Cards,
    Modules,
    Clients,
    All,
}

impl ListEntity {
    fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sinks" => Some(Self::Sinks),
            "sources" => Some(Self::Sources),
            "sink-inputs" => Some(Self::SinkInputs),
            "source-outputs" => Some(Self::SourceOutputs),
            "cards" => Some(Self::Cards),
            "modules" => Some(Self::Modules),
            "clients" => Some(Self::Clients),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// pactl implementation
// ---------------------------------------------------------------------------

fn run_pactl(args: &[String]) -> i32 {
    if args.is_empty() {
        print_pactl_usage();
        return 1;
    }

    // Check for --help and --version flags
    for a in args {
        if a == "--help" || a == "-h" {
            print_pactl_usage();
            return 0;
        }
        if a == "--version" {
            println!("pactl {VERSION}");
            return 0;
        }
    }

    let cmd_str = &args[0];
    let cmd = match PactlCommand::from_str_opt(cmd_str) {
        Some(c) => c,
        None => {
            eprintln!("pactl: unknown command '{cmd_str}'");
            return 1;
        }
    };

    let cmd_args = &args[1..];
    let mut state = PulseState::default_state();

    match cmd {
        PactlCommand::List => run_pactl_list(cmd_args, &state),
        PactlCommand::Info => run_pactl_info(&state),
        PactlCommand::Stat => run_pactl_stat(&state),
        PactlCommand::SetSinkVolume => run_pactl_set_sink_volume(cmd_args, &mut state),
        PactlCommand::SetSourceVolume => run_pactl_set_source_volume(cmd_args, &mut state),
        PactlCommand::SetSinkMute => run_pactl_set_sink_mute(cmd_args, &mut state),
        PactlCommand::SetSourceMute => run_pactl_set_source_mute(cmd_args, &mut state),
        PactlCommand::SetDefaultSink => run_pactl_set_default_sink(cmd_args, &mut state),
        PactlCommand::SetDefaultSource => run_pactl_set_default_source(cmd_args, &mut state),
        PactlCommand::MoveSinkInput => run_pactl_move_sink_input(cmd_args, &mut state),
        PactlCommand::MoveSourceOutput => run_pactl_move_source_output(cmd_args, &mut state),
        PactlCommand::LoadModule => run_pactl_load_module(cmd_args, &mut state),
        PactlCommand::UnloadModule => run_pactl_unload_module(cmd_args, &mut state),
        PactlCommand::SetCardProfile => run_pactl_set_card_profile(cmd_args, &mut state),
        PactlCommand::Subscribe => run_pactl_subscribe(),
        PactlCommand::Exit => run_pactl_exit(),
    }
}

fn print_pactl_usage() {
    println!("pactl {VERSION} - PulseAudio control tool");
    println!();
    println!("Usage: pactl [options] <command> [args]");
    println!();
    println!("Commands:");
    println!("  list [type]                List objects (sinks, sources, sink-inputs,");
    println!("                             source-outputs, cards, modules, clients)");
    println!("  info                       Show server info");
    println!("  stat                       Show memory statistics");
    println!("  set-sink-volume SINK VOL   Set sink volume");
    println!("  set-source-volume SRC VOL  Set source volume");
    println!("  set-sink-mute SINK MUTE    Set sink mute (1/0/toggle)");
    println!("  set-source-mute SRC MUTE   Set source mute (1/0/toggle)");
    println!("  set-default-sink SINK      Set the default sink");
    println!("  set-default-source SRC     Set the default source");
    println!("  move-sink-input IDX SINK   Move sink input to a different sink");
    println!("  move-source-output IDX SRC Move source output to a different source");
    println!("  load-module NAME [ARGS]    Load a module");
    println!("  unload-module IDX          Unload a module");
    println!("  set-card-profile CARD PROF Set card profile");
    println!("  subscribe                  Subscribe to events");
    println!("  exit                       Shut down the daemon");
    println!();
    println!("Options:");
    println!("  -h, --help     Show this help");
    println!("  --version      Show version");
}

fn run_pactl_list(args: &[String], state: &PulseState) -> i32 {
    let entity = if args.is_empty() {
        ListEntity::All
    } else {
        match ListEntity::from_str_opt(&args[0]) {
            Some(e) => e,
            None => {
                eprintln!("pactl: invalid list type '{}'", args[0]);
                return 1;
            }
        }
    };

    let short = args.iter().any(|a| a == "short");

    match entity {
        ListEntity::Sinks => print_sinks(&state.sinks, short),
        ListEntity::Sources => print_sources(&state.sources, short),
        ListEntity::SinkInputs => print_sink_inputs(&state.sink_inputs, short),
        ListEntity::SourceOutputs => print_source_outputs(&state.source_outputs, short),
        ListEntity::Cards => print_cards(&state.cards, short),
        ListEntity::Modules => print_modules(&state.modules, short),
        ListEntity::Clients => print_clients(&state.clients, short),
        ListEntity::All => {
            print_sinks(&state.sinks, short);
            print_sources(&state.sources, short);
            print_sink_inputs(&state.sink_inputs, short);
            print_source_outputs(&state.source_outputs, short);
            print_cards(&state.cards, short);
            print_modules(&state.modules, short);
            print_clients(&state.clients, short);
        }
    }
    0
}

fn print_sinks(sinks: &[Sink], short: bool) {
    if short {
        for s in sinks {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                s.index,
                s.name,
                s.driver,
                s.sample_spec,
                s.state.as_str()
            );
        }
    } else {
        for s in sinks {
            println!("Sink #{}", s.index);
            print!("{}", s.format_info());
            println!();
        }
    }
}

fn print_sources(sources: &[Source], short: bool) {
    if short {
        for s in sources {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                s.index,
                s.name,
                s.driver,
                s.sample_spec,
                s.state.as_str()
            );
        }
    } else {
        for s in sources {
            println!("Source #{}", s.index);
            print!("{}", s.format_info());
            println!();
        }
    }
}

fn print_sink_inputs(inputs: &[SinkInput], short: bool) {
    if short {
        for si in inputs {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                si.index, si.sink, si.name, si.driver, si.sample_spec,
            );
        }
    } else {
        for si in inputs {
            println!("Sink Input #{}", si.index);
            print!("{}", si.format_info());
            println!();
        }
    }
}

fn print_source_outputs(outputs: &[SourceOutput], short: bool) {
    if short {
        for so in outputs {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                so.index, so.source, so.name, so.driver, so.sample_spec,
            );
        }
    } else {
        for so in outputs {
            println!("Source Output #{}", so.index);
            print!("{}", so.format_info());
            println!();
        }
    }
}

fn print_cards(cards: &[Card], short: bool) {
    if short {
        for c in cards {
            println!(
                "{}\t{}\t{}\t{}",
                c.index, c.name, c.driver, c.active_profile
            );
        }
    } else {
        for c in cards {
            println!("Card #{}", c.index);
            print!("{}", c.format_info());
            println!();
        }
    }
}

fn print_modules(modules: &[Module], short: bool) {
    if short {
        for m in modules {
            println!(
                "{}\t{}\t{}",
                m.index,
                m.name,
                if m.argument.is_empty() {
                    "<none>"
                } else {
                    &m.argument
                }
            );
        }
    } else {
        for m in modules {
            println!("Module #{}", m.index);
            print!("{}", m.format_info());
            println!();
        }
    }
}

fn print_clients(clients: &[Client], short: bool) {
    if short {
        for c in clients {
            println!("{}\t{}\t{}", c.index, c.name, c.driver);
        }
    } else {
        for c in clients {
            println!("Client #{}", c.index);
            print!("{}", c.format_info());
            println!();
        }
    }
}

fn run_pactl_info(state: &PulseState) -> i32 {
    print!("{}", state.server.format_info());
    0
}

fn run_pactl_stat(state: &PulseState) -> i32 {
    print!("{}", state.stat.format_info());
    0
}

fn run_pactl_set_sink_volume(args: &[String], state: &mut PulseState) -> i32 {
    if args.len() < 2 {
        eprintln!("pactl: set-sink-volume requires SINK and VOLUME arguments");
        return 1;
    }
    let sink_id = &args[0];
    let vol_str = &args[1];

    let pos = match state.find_sink_by_name_or_index(sink_id) {
        Some(p) => p,
        None => {
            eprintln!("pactl: sink '{sink_id}' not found");
            return 1;
        }
    };

    let current_avg = state.sinks[pos].volume.average();
    match parse_volume(vol_str, current_avg) {
        Ok(v) => {
            state.sinks[pos].volume.set_all(v);
            0
        }
        Err(e) => {
            eprintln!("pactl: {e}");
            1
        }
    }
}

fn run_pactl_set_source_volume(args: &[String], state: &mut PulseState) -> i32 {
    if args.len() < 2 {
        eprintln!("pactl: set-source-volume requires SOURCE and VOLUME arguments");
        return 1;
    }
    let source_id = &args[0];
    let vol_str = &args[1];

    let pos = match state.find_source_by_name_or_index(source_id) {
        Some(p) => p,
        None => {
            eprintln!("pactl: source '{source_id}' not found");
            return 1;
        }
    };

    let current_avg = state.sources[pos].volume.average();
    match parse_volume(vol_str, current_avg) {
        Ok(v) => {
            state.sources[pos].volume.set_all(v);
            0
        }
        Err(e) => {
            eprintln!("pactl: {e}");
            1
        }
    }
}

fn run_pactl_set_sink_mute(args: &[String], state: &mut PulseState) -> i32 {
    if args.len() < 2 {
        eprintln!("pactl: set-sink-mute requires SINK and MUTE arguments");
        return 1;
    }
    let sink_id = &args[0];
    let mute_str = &args[1];

    let pos = match state.find_sink_by_name_or_index(sink_id) {
        Some(p) => p,
        None => {
            eprintln!("pactl: sink '{sink_id}' not found");
            return 1;
        }
    };

    match parse_mute(mute_str, state.sinks[pos].muted) {
        Ok(m) => {
            state.sinks[pos].muted = m;
            0
        }
        Err(e) => {
            eprintln!("pactl: {e}");
            1
        }
    }
}

fn run_pactl_set_source_mute(args: &[String], state: &mut PulseState) -> i32 {
    if args.len() < 2 {
        eprintln!("pactl: set-source-mute requires SOURCE and MUTE arguments");
        return 1;
    }
    let source_id = &args[0];
    let mute_str = &args[1];

    let pos = match state.find_source_by_name_or_index(source_id) {
        Some(p) => p,
        None => {
            eprintln!("pactl: source '{source_id}' not found");
            return 1;
        }
    };

    match parse_mute(mute_str, state.sources[pos].muted) {
        Ok(m) => {
            state.sources[pos].muted = m;
            0
        }
        Err(e) => {
            eprintln!("pactl: {e}");
            1
        }
    }
}

fn run_pactl_set_default_sink(args: &[String], state: &mut PulseState) -> i32 {
    if args.is_empty() {
        eprintln!("pactl: set-default-sink requires a SINK argument");
        return 1;
    }
    let sink_id = &args[0];

    // Verify sink exists
    if state.find_sink_by_name_or_index(sink_id).is_none() {
        eprintln!("pactl: sink '{sink_id}' not found");
        return 1;
    }

    // If numeric index was given, resolve to the sink name
    if let Ok(idx) = sink_id.parse::<u32>()
        && let Some(pos) = state.sinks.iter().position(|s| s.index == idx)
    {
        state.server.default_sink_name = state.sinks[pos].name.clone();
        return 0;
    }
    state.server.default_sink_name = sink_id.clone();
    0
}

fn run_pactl_set_default_source(args: &[String], state: &mut PulseState) -> i32 {
    if args.is_empty() {
        eprintln!("pactl: set-default-source requires a SOURCE argument");
        return 1;
    }
    let source_id = &args[0];

    if state.find_source_by_name_or_index(source_id).is_none() {
        eprintln!("pactl: source '{source_id}' not found");
        return 1;
    }

    if let Ok(idx) = source_id.parse::<u32>()
        && let Some(pos) = state.sources.iter().position(|s| s.index == idx)
    {
        state.server.default_source_name = state.sources[pos].name.clone();
        return 0;
    }
    state.server.default_source_name = source_id.clone();
    0
}

fn run_pactl_move_sink_input(args: &[String], state: &mut PulseState) -> i32 {
    if args.len() < 2 {
        eprintln!("pactl: move-sink-input requires INPUT_INDEX and SINK arguments");
        return 1;
    }
    let input_idx: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pactl: invalid sink input index '{}'", args[0]);
            return 1;
        }
    };
    let sink_id = &args[1];

    let si_pos = match state.find_sink_input_by_index(input_idx) {
        Some(p) => p,
        None => {
            eprintln!("pactl: sink input {input_idx} not found");
            return 1;
        }
    };

    let sink_pos = match state.find_sink_by_name_or_index(sink_id) {
        Some(p) => p,
        None => {
            eprintln!("pactl: sink '{sink_id}' not found");
            return 1;
        }
    };

    state.sink_inputs[si_pos].sink = state.sinks[sink_pos].index;
    0
}

fn run_pactl_move_source_output(args: &[String], state: &mut PulseState) -> i32 {
    if args.len() < 2 {
        eprintln!("pactl: move-source-output requires OUTPUT_INDEX and SOURCE arguments");
        return 1;
    }
    let output_idx: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pactl: invalid source output index '{}'", args[0]);
            return 1;
        }
    };
    let source_id = &args[1];

    let so_pos = match state.find_source_output_by_index(output_idx) {
        Some(p) => p,
        None => {
            eprintln!("pactl: source output {output_idx} not found");
            return 1;
        }
    };

    let source_pos = match state.find_source_by_name_or_index(source_id) {
        Some(p) => p,
        None => {
            eprintln!("pactl: source '{source_id}' not found");
            return 1;
        }
    };

    state.source_outputs[so_pos].source = state.sources[source_pos].index;
    0
}

fn run_pactl_load_module(args: &[String], state: &mut PulseState) -> i32 {
    if args.is_empty() {
        eprintln!("pactl: load-module requires a module name");
        return 1;
    }
    let name = &args[0];
    let argument = if args.len() > 1 {
        args[1..].join(" ")
    } else {
        String::new()
    };

    if state.modules.len() >= MAX_MODULES {
        eprintln!("pactl: maximum number of modules reached");
        return 1;
    }

    let idx = state.next_module_index;
    state.next_module_index += 1;

    state.modules.push(Module {
        index: idx,
        name: name.clone(),
        argument,
        _n_used: -1,
    });

    println!("{idx}");
    0
}

fn run_pactl_unload_module(args: &[String], state: &mut PulseState) -> i32 {
    if args.is_empty() {
        eprintln!("pactl: unload-module requires a module index");
        return 1;
    }
    let idx: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pactl: invalid module index '{}'", args[0]);
            return 1;
        }
    };

    let pos = match state.find_module_by_index(idx) {
        Some(p) => p,
        None => {
            eprintln!("pactl: module {idx} not found");
            return 1;
        }
    };

    state.modules.remove(pos);
    0
}

fn run_pactl_set_card_profile(args: &[String], state: &mut PulseState) -> i32 {
    if args.len() < 2 {
        eprintln!("pactl: set-card-profile requires CARD and PROFILE arguments");
        return 1;
    }
    let card_id = &args[0];
    let profile_name = &args[1];

    let pos = match state.find_card_by_name_or_index(card_id) {
        Some(p) => p,
        None => {
            eprintln!("pactl: card '{card_id}' not found");
            return 1;
        }
    };

    let has_profile = state.cards[pos]
        .profiles
        .iter()
        .any(|p| p.name == *profile_name);

    if !has_profile {
        eprintln!("pactl: profile '{profile_name}' not found on card '{card_id}'");
        return 1;
    }

    state.cards[pos].active_profile = profile_name.clone();
    0
}

fn run_pactl_subscribe() -> i32 {
    println!("Now subscribed to events.");
    println!("Event 'change' on sink #0");
    println!("Event 'new' on client #1");
    println!("Event 'remove' on client #1");
    0
}

fn run_pactl_exit() -> i32 {
    println!("Requesting daemon exit...");
    0
}

// ---------------------------------------------------------------------------
// pacmd implementation
// ---------------------------------------------------------------------------

fn run_pacmd(args: &[String]) -> i32 {
    // pacmd accepts a command as args, or interactive mode (not supported here)
    for a in args {
        if a == "--help" || a == "-h" {
            print_pacmd_usage();
            return 0;
        }
        if a == "--version" {
            println!("pacmd {VERSION}");
            return 0;
        }
    }

    if args.is_empty() {
        println!("Welcome to PulseAudio {VERSION}! Use \"help\" for usage information.");
        println!(">>> (interactive mode not supported in SlateOS simulation)");
        return 0;
    }

    let cmd = &args[0];
    let cmd_args = &args[1..];
    let mut state = PulseState::default_state();

    match cmd.as_str() {
        "help" => {
            print_pacmd_help();
            0
        }
        "list-sinks" => {
            pacmd_list_sinks(&state);
            0
        }
        "list-sources" => {
            pacmd_list_sources(&state);
            0
        }
        "list-modules" => {
            pacmd_list_modules(&state);
            0
        }
        "list-clients" => {
            pacmd_list_clients(&state);
            0
        }
        "list-cards" => {
            pacmd_list_cards(&state);
            0
        }
        "list-sink-inputs" => {
            pacmd_list_sink_inputs(&state);
            0
        }
        "list-source-outputs" => {
            pacmd_list_source_outputs(&state);
            0
        }
        "stat" => {
            run_pactl_stat(&state);
            0
        }
        "info" | "dump" => {
            pacmd_dump(&state);
            0
        }
        "set-default-sink" => {
            if cmd_args.is_empty() {
                eprintln!("pacmd: set-default-sink requires a sink name");
                return 1;
            }
            run_pactl_set_default_sink(cmd_args, &mut state)
        }
        "set-default-source" => {
            if cmd_args.is_empty() {
                eprintln!("pacmd: set-default-source requires a source name");
                return 1;
            }
            run_pactl_set_default_source(cmd_args, &mut state)
        }
        "set-sink-volume" => {
            if cmd_args.len() < 2 {
                eprintln!("pacmd: set-sink-volume requires SINK and VOLUME");
                return 1;
            }
            run_pactl_set_sink_volume(cmd_args, &mut state)
        }
        "set-source-volume" => {
            if cmd_args.len() < 2 {
                eprintln!("pacmd: set-source-volume requires SOURCE and VOLUME");
                return 1;
            }
            run_pactl_set_source_volume(cmd_args, &mut state)
        }
        "set-sink-mute" => {
            if cmd_args.len() < 2 {
                eprintln!("pacmd: set-sink-mute requires SINK and MUTE");
                return 1;
            }
            run_pactl_set_sink_mute(cmd_args, &mut state)
        }
        "set-source-mute" => {
            if cmd_args.len() < 2 {
                eprintln!("pacmd: set-source-mute requires SOURCE and MUTE");
                return 1;
            }
            run_pactl_set_source_mute(cmd_args, &mut state)
        }
        "set-card-profile" => run_pactl_set_card_profile(cmd_args, &mut state),
        "load-module" => run_pactl_load_module(cmd_args, &mut state),
        "unload-module" => run_pactl_unload_module(cmd_args, &mut state),
        "exit" => {
            println!("Exiting.");
            0
        }
        _ => {
            eprintln!("pacmd: unknown command '{cmd}'");
            1
        }
    }
}

fn print_pacmd_usage() {
    println!("pacmd {VERSION} - PulseAudio command-line daemon interface");
    println!();
    println!("Usage: pacmd [command] [args]");
    println!();
    println!("Without arguments, enters interactive mode.");
    println!("Use 'pacmd help' for a list of commands.");
}

fn print_pacmd_help() {
    println!("Available commands:");
    println!("  help                       Show this help");
    println!("  list-sinks                 List sinks");
    println!("  list-sources               List sources");
    println!("  list-modules               List modules");
    println!("  list-clients               List clients");
    println!("  list-cards                 List cards");
    println!("  list-sink-inputs           List sink inputs");
    println!("  list-source-outputs        List source outputs");
    println!("  stat                       Show statistics");
    println!("  info                       Dump server state");
    println!("  dump                       Dump server state");
    println!("  set-default-sink NAME      Set the default sink");
    println!("  set-default-source NAME    Set the default source");
    println!("  set-sink-volume SINK VOL   Set sink volume");
    println!("  set-source-volume SRC VOL  Set source volume");
    println!("  set-sink-mute SINK BOOL    Set sink mute state");
    println!("  set-source-mute SRC BOOL   Set source mute state");
    println!("  set-card-profile CARD PROF Set card profile");
    println!("  load-module NAME [ARGS]    Load a module");
    println!("  unload-module IDX          Unload a module");
    println!("  exit                       Terminate the daemon");
}

fn pacmd_list_sinks(state: &PulseState) {
    println!("{} sink(s) available.", state.sinks.len());
    for s in &state.sinks {
        print!("{}", s.format_info());
    }
}

fn pacmd_list_sources(state: &PulseState) {
    println!("{} source(s) available.", state.sources.len());
    for s in &state.sources {
        print!("{}", s.format_info());
    }
}

fn pacmd_list_modules(state: &PulseState) {
    println!("{} module(s) loaded.", state.modules.len());
    for m in &state.modules {
        print!("{}", m.format_info());
    }
}

fn pacmd_list_clients(state: &PulseState) {
    println!("{} client(s).", state.clients.len());
    for c in &state.clients {
        print!("{}", c.format_info());
    }
}

fn pacmd_list_cards(state: &PulseState) {
    println!("{} card(s) available.", state.cards.len());
    for c in &state.cards {
        print!("{}", c.format_info());
    }
}

fn pacmd_list_sink_inputs(state: &PulseState) {
    println!("{} sink input(s) available.", state.sink_inputs.len());
    for si in &state.sink_inputs {
        print!("{}", si.format_info());
    }
}

fn pacmd_list_source_outputs(state: &PulseState) {
    println!("{} source output(s) available.", state.source_outputs.len());
    for so in &state.source_outputs {
        print!("{}", so.format_info());
    }
}

fn pacmd_dump(state: &PulseState) {
    println!("### Configuration dump generated at simulated time ###");
    println!();
    // Default sink/source
    println!("set-default-sink {}", state.server.default_sink_name);
    println!("set-default-source {}", state.server.default_source_name);
    println!();
    // Sink volumes and mutes
    for s in &state.sinks {
        println!("set-sink-volume {} 0x{:x}", s.name, s.volume.average());
        println!(
            "set-sink-mute {} {}",
            s.name,
            if s.muted { "yes" } else { "no" }
        );
    }
    // Source volumes and mutes
    for s in &state.sources {
        println!("set-source-volume {} 0x{:x}", s.name, s.volume.average());
        println!(
            "set-source-mute {} {}",
            s.name,
            if s.muted { "yes" } else { "no" }
        );
    }
    // Card profiles
    for c in &state.cards {
        println!("set-card-profile {} {}", c.name, c.active_profile);
    }
}

// ---------------------------------------------------------------------------
// paplay implementation
// ---------------------------------------------------------------------------

/// Options for the paplay personality.
#[derive(Clone, Debug)]
struct PaplayOptions {
    filename: Option<String>,
    device: Option<String>,
    channels: u16,
    rate: u32,
    format: SampleFormat,
    volume: Option<u32>,
    _latency_msec: u32,
    _process_time_msec: u32,
    _raw: bool,
    list_file_formats: bool,
}

impl PaplayOptions {
    fn new() -> Self {
        Self {
            filename: None,
            device: None,
            channels: DEFAULT_CHANNELS,
            rate: DEFAULT_SAMPLE_RATE,
            format: SampleFormat::S16Le,
            volume: None,
            _latency_msec: 200,
            _process_time_msec: 0,
            _raw: false,
            list_file_formats: false,
        }
    }
}

fn run_paplay(args: &[String]) -> i32 {
    let mut opts = PaplayOptions::new();
    let mut i = 0;

    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--help" | "-h" => {
                print_paplay_usage();
                return 0;
            }
            "--version" => {
                println!("paplay {VERSION}");
                return 0;
            }
            "-d" | "--device" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("paplay: --device requires an argument");
                    return 1;
                }
                opts.device = Some(args[i].clone());
            }
            "--channels" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("paplay: --channels requires an argument");
                    return 1;
                }
                opts.channels = match args[i].parse() {
                    Ok(c) => c,
                    Err(_) => {
                        eprintln!("paplay: invalid channel count '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--rate" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("paplay: --rate requires an argument");
                    return 1;
                }
                opts.rate = match args[i].parse() {
                    Ok(r) => r,
                    Err(_) => {
                        eprintln!("paplay: invalid sample rate '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("paplay: --format requires an argument");
                    return 1;
                }
                opts.format = match SampleFormat::from_str_opt(&args[i]) {
                    Some(f) => f,
                    None => {
                        eprintln!("paplay: unknown format '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--volume" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("paplay: --volume requires an argument");
                    return 1;
                }
                opts.volume = match args[i].parse() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        eprintln!("paplay: invalid volume '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--latency-msec" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("paplay: --latency-msec requires an argument");
                    return 1;
                }
                opts._latency_msec = match args[i].parse() {
                    Ok(l) => l,
                    Err(_) => {
                        eprintln!("paplay: invalid latency '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--process-time-msec" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("paplay: --process-time-msec requires an argument");
                    return 1;
                }
                opts._process_time_msec = match args[i].parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("paplay: invalid process time '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--raw" => {
                opts._raw = true;
            }
            "--list-file-formats" => {
                opts.list_file_formats = true;
            }
            _ => {
                if a.starts_with('-') {
                    eprintln!("paplay: unknown option '{a}'");
                    return 1;
                }
                opts.filename = Some(a.clone());
            }
        }
        i += 1;
    }

    if opts.list_file_formats {
        println!("Supported file formats:");
        println!("  wav   - Microsoft WAV");
        println!("  au    - Sun/NeXT AU");
        println!("  flac  - FLAC Lossless Audio");
        println!("  ogg   - Ogg Vorbis");
        return 0;
    }

    let filename = match &opts.filename {
        Some(f) => f.clone(),
        None => {
            eprintln!("paplay: no file specified");
            return 1;
        }
    };

    let spec = SampleSpec::new(opts.format, opts.rate, opts.channels);
    let bps = spec.bytes_per_second();
    let device_name = opts.device.as_deref().unwrap_or("default sink");

    println!("Playing '{filename}' on '{device_name}'...");
    println!(
        "  Format: {}, {} channels, {} Hz",
        opts.format.as_str(),
        opts.channels,
        opts.rate
    );
    println!("  Estimated throughput: {} bytes/sec", bps);
    if let Some(v) = opts.volume {
        let pct = (u64::from(v) * 100) / u64::from(VOLUME_NORM);
        println!("  Volume: {v} ({pct}%)");
    }
    println!("Playback complete (simulated).");
    0
}

fn print_paplay_usage() {
    println!("paplay {VERSION} - PulseAudio playback tool");
    println!();
    println!("Usage: paplay [options] <FILE>");
    println!();
    println!("Options:");
    println!("  -h, --help               Show this help");
    println!("  --version                Show version");
    println!("  -d, --device=DEVICE      Playback device");
    println!("  --channels=N             Number of channels");
    println!("  --rate=RATE              Sample rate in Hz");
    println!("  --format=FORMAT          Sample format (s16le, float32le, etc.)");
    println!("  --volume=VOL             Volume (0-65536)");
    println!("  --latency-msec=MSEC      Requested latency");
    println!("  --process-time-msec=MSEC Requested process time");
    println!("  --raw                    Raw PCM data (no header)");
    println!("  --list-file-formats      List supported file formats");
}

// ---------------------------------------------------------------------------
// parecord implementation
// ---------------------------------------------------------------------------

/// Options for the parecord personality.
#[derive(Clone, Debug)]
struct ParecordOptions {
    filename: Option<String>,
    device: Option<String>,
    channels: u16,
    rate: u32,
    format: SampleFormat,
    volume: Option<u32>,
    _latency_msec: u32,
    _process_time_msec: u32,
    _raw: bool,
    _file_format: String,
    _fix_channels: bool,
    _fix_rate: bool,
    _fix_format: bool,
    _monitor_stream: Option<u32>,
}

impl ParecordOptions {
    fn new() -> Self {
        Self {
            filename: None,
            device: None,
            channels: DEFAULT_CHANNELS,
            rate: DEFAULT_SAMPLE_RATE,
            format: SampleFormat::S16Le,
            volume: None,
            _latency_msec: 200,
            _process_time_msec: 0,
            _raw: false,
            _file_format: "wav".to_string(),
            _fix_channels: false,
            _fix_rate: false,
            _fix_format: false,
            _monitor_stream: None,
        }
    }
}

fn run_parecord(args: &[String]) -> i32 {
    let mut opts = ParecordOptions::new();
    let mut i = 0;

    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--help" | "-h" => {
                print_parecord_usage();
                return 0;
            }
            "--version" => {
                println!("parecord {VERSION}");
                return 0;
            }
            "-d" | "--device" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --device requires an argument");
                    return 1;
                }
                opts.device = Some(args[i].clone());
            }
            "--channels" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --channels requires an argument");
                    return 1;
                }
                opts.channels = match args[i].parse() {
                    Ok(c) => c,
                    Err(_) => {
                        eprintln!("parecord: invalid channel count '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--rate" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --rate requires an argument");
                    return 1;
                }
                opts.rate = match args[i].parse() {
                    Ok(r) => r,
                    Err(_) => {
                        eprintln!("parecord: invalid sample rate '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --format requires an argument");
                    return 1;
                }
                opts.format = match SampleFormat::from_str_opt(&args[i]) {
                    Some(f) => f,
                    None => {
                        eprintln!("parecord: unknown format '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--volume" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --volume requires an argument");
                    return 1;
                }
                opts.volume = match args[i].parse() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        eprintln!("parecord: invalid volume '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--latency-msec" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --latency-msec requires an argument");
                    return 1;
                }
                opts._latency_msec = match args[i].parse() {
                    Ok(l) => l,
                    Err(_) => {
                        eprintln!("parecord: invalid latency '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--process-time-msec" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --process-time-msec requires an argument");
                    return 1;
                }
                opts._process_time_msec = match args[i].parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("parecord: invalid process time '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--raw" => {
                opts._raw = true;
            }
            "--file-format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --file-format requires an argument");
                    return 1;
                }
                opts._file_format = args[i].clone();
            }
            "--fix-channels" => {
                opts._fix_channels = true;
            }
            "--fix-rate" => {
                opts._fix_rate = true;
            }
            "--fix-format" => {
                opts._fix_format = true;
            }
            "--monitor-stream" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("parecord: --monitor-stream requires an argument");
                    return 1;
                }
                opts._monitor_stream = match args[i].parse() {
                    Ok(m) => Some(m),
                    Err(_) => {
                        eprintln!("parecord: invalid stream index '{}'", args[i]);
                        return 1;
                    }
                };
            }
            _ => {
                if a.starts_with('-') {
                    eprintln!("parecord: unknown option '{a}'");
                    return 1;
                }
                opts.filename = Some(a.clone());
            }
        }
        i += 1;
    }

    let filename = match &opts.filename {
        Some(f) => f.clone(),
        None => {
            eprintln!("parecord: no file specified");
            return 1;
        }
    };

    let spec = SampleSpec::new(opts.format, opts.rate, opts.channels);
    let bps = spec.bytes_per_second();
    let device_name = opts.device.as_deref().unwrap_or("default source");

    println!("Recording to '{filename}' from '{device_name}'...");
    println!(
        "  Format: {}, {} channels, {} Hz",
        opts.format.as_str(),
        opts.channels,
        opts.rate
    );
    println!("  File format: {}", opts._file_format);
    println!("  Estimated throughput: {} bytes/sec", bps);
    if let Some(v) = opts.volume {
        let pct = (u64::from(v) * 100) / u64::from(VOLUME_NORM);
        println!("  Volume: {v} ({pct}%)");
    }
    println!("Recording complete (simulated, 0 bytes captured).");
    0
}

fn print_parecord_usage() {
    println!("parecord {VERSION} - PulseAudio recording tool");
    println!();
    println!("Usage: parecord [options] <FILE>");
    println!();
    println!("Options:");
    println!("  -h, --help               Show this help");
    println!("  --version                Show version");
    println!("  -d, --device=DEVICE      Recording device");
    println!("  --channels=N             Number of channels");
    println!("  --rate=RATE              Sample rate in Hz");
    println!("  --format=FORMAT          Sample format");
    println!("  --volume=VOL             Volume (0-65536)");
    println!("  --latency-msec=MSEC      Requested latency");
    println!("  --process-time-msec=MSEC Requested process time");
    println!("  --raw                    Raw PCM data");
    println!("  --file-format=FORMAT     Output file format (wav, au, etc.)");
    println!("  --fix-channels           Use fixed channel count");
    println!("  --fix-rate               Use fixed sample rate");
    println!("  --fix-format             Use fixed sample format");
    println!("  --monitor-stream=IDX     Monitor a specific stream");
}

// ---------------------------------------------------------------------------
// pasuspender implementation
// ---------------------------------------------------------------------------

fn run_pasuspender(args: &[String]) -> i32 {
    for a in args {
        if a == "--help" || a == "-h" {
            print_pasuspender_usage();
            return 0;
        }
        if a == "--version" {
            println!("pasuspender {VERSION}");
            return 0;
        }
    }

    // Parse -- and the command after it
    let mut server: Option<String> = None;
    let mut cmd_start: Option<usize> = None;
    let mut i = 0;

    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-s" | "--server" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pasuspender: --server requires an argument");
                    return 1;
                }
                server = Some(args[i].clone());
            }
            "--" => {
                cmd_start = Some(i + 1);
                break;
            }
            _ => {
                // First non-option is the command
                cmd_start = Some(i);
                break;
            }
        }
        i += 1;
    }

    let command = match cmd_start {
        Some(idx) if idx < args.len() => args[idx..].join(" "),
        _ => {
            eprintln!("pasuspender: no command specified");
            return 1;
        }
    };

    let server_name = server
        .as_deref()
        .unwrap_or("unix:/run/user/1000/pulse/native");
    println!("Suspending PulseAudio on server '{server_name}'...");
    println!("Running: {command}");
    println!("Command completed (simulated).");
    println!("Resuming PulseAudio...");
    0
}

fn print_pasuspender_usage() {
    println!("pasuspender {VERSION} - Temporarily suspend PulseAudio");
    println!();
    println!("Usage: pasuspender [options] -- <COMMAND>");
    println!();
    println!("Options:");
    println!("  -h, --help               Show this help");
    println!("  --version                Show version");
    println!("  -s, --server=SERVER      Server to connect to");
}

// ---------------------------------------------------------------------------
// pulseaudio daemon implementation
// ---------------------------------------------------------------------------

/// Daemon options.
#[derive(Clone, Debug)]
struct DaemonOptions {
    _daemonize: bool,
    _system: bool,
    _realtime: bool,
    _disallow_exit: bool,
    _disallow_module_loading: bool,
    _log_level: u32,
    _log_target: String,
    _high_priority: bool,
    _exit_idle_time: i32,
    _scache_idle_time: i32,
    check: bool,
    kill: bool,
    start: bool,
    dump_conf: bool,
    dump_modules: bool,
    dump_resample_methods: bool,
    cleanup_shm: bool,
    _dl_search_path: Option<String>,
}

impl DaemonOptions {
    fn new() -> Self {
        Self {
            _daemonize: false,
            _system: false,
            _realtime: true,
            _disallow_exit: false,
            _disallow_module_loading: false,
            _log_level: 1,
            _log_target: "auto".to_string(),
            _high_priority: true,
            _exit_idle_time: 20,
            _scache_idle_time: 20,
            check: false,
            kill: false,
            start: false,
            dump_conf: false,
            dump_modules: false,
            dump_resample_methods: false,
            cleanup_shm: false,
            _dl_search_path: None,
        }
    }
}

fn run_pulseaudio(args: &[String]) -> i32 {
    let mut opts = DaemonOptions::new();
    let mut i = 0;

    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--help" | "-h" => {
                print_pulseaudio_usage();
                return 0;
            }
            "--version" => {
                println!("pulseaudio {VERSION}");
                return 0;
            }
            "-D" | "--daemonize" => {
                opts._daemonize = true;
            }
            "--system" => {
                opts._system = true;
            }
            "--realtime" => {
                opts._realtime = true;
            }
            "--no-realtime" => {
                opts._realtime = false;
            }
            "--disallow-exit" => {
                opts._disallow_exit = true;
            }
            "--disallow-module-loading" => {
                opts._disallow_module_loading = true;
            }
            "--high-priority" => {
                opts._high_priority = true;
            }
            "--no-high-priority" => {
                opts._high_priority = false;
            }
            "--log-level" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pulseaudio: --log-level requires an argument");
                    return 1;
                }
                opts._log_level = match args[i].parse() {
                    Ok(l) => l,
                    Err(_) => {
                        eprintln!("pulseaudio: invalid log level '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--log-target" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pulseaudio: --log-target requires an argument");
                    return 1;
                }
                opts._log_target = args[i].clone();
            }
            "--exit-idle-time" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pulseaudio: --exit-idle-time requires an argument");
                    return 1;
                }
                opts._exit_idle_time = match args[i].parse() {
                    Ok(t) => t,
                    Err(_) => {
                        eprintln!("pulseaudio: invalid idle time '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--scache-idle-time" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pulseaudio: --scache-idle-time requires an argument");
                    return 1;
                }
                opts._scache_idle_time = match args[i].parse() {
                    Ok(t) => t,
                    Err(_) => {
                        eprintln!("pulseaudio: invalid scache idle time '{}'", args[i]);
                        return 1;
                    }
                };
            }
            "--check" => {
                opts.check = true;
            }
            "-k" | "--kill" => {
                opts.kill = true;
            }
            "--start" => {
                opts.start = true;
            }
            "--dump-conf" => {
                opts.dump_conf = true;
            }
            "--dump-modules" => {
                opts.dump_modules = true;
            }
            "--dump-resample-methods" => {
                opts.dump_resample_methods = true;
            }
            "--cleanup-shm" => {
                opts.cleanup_shm = true;
            }
            "--dl-search-path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pulseaudio: --dl-search-path requires an argument");
                    return 1;
                }
                opts._dl_search_path = Some(args[i].clone());
            }
            _ => {
                if a.starts_with('-') {
                    eprintln!("pulseaudio: unknown option '{a}'");
                    return 1;
                }
            }
        }
        i += 1;
    }

    if opts.check {
        println!("Daemon is running (simulated).");
        return 0;
    }
    if opts.kill {
        println!("Killing daemon (simulated).");
        return 0;
    }
    if opts.dump_conf {
        dump_daemon_conf(&opts);
        return 0;
    }
    if opts.dump_modules {
        dump_available_modules();
        return 0;
    }
    if opts.dump_resample_methods {
        dump_resample_methods();
        return 0;
    }
    if opts.cleanup_shm {
        println!("Cleaning up shared memory segments (simulated).");
        return 0;
    }
    if opts.start {
        println!("Starting PulseAudio daemon (simulated)...");
        println!("Daemon startup complete.");
        return 0;
    }

    // Default: run as daemon
    println!("PulseAudio {VERSION} starting up.");
    println!("Using sample spec: s16le 2ch 44100Hz");
    println!("Using channel map: front-left,front-right");
    println!("Running in foreground mode (simulated).");
    println!("Daemon ready.");
    0
}

fn dump_daemon_conf(opts: &DaemonOptions) {
    println!("### Daemon configuration dump ###");
    println!("daemonize = {}", if opts._daemonize { "yes" } else { "no" });
    println!(
        "system-instance = {}",
        if opts._system { "yes" } else { "no" }
    );
    println!(
        "realtime-scheduling = {}",
        if opts._realtime { "yes" } else { "no" }
    );
    println!(
        "high-priority = {}",
        if opts._high_priority { "yes" } else { "no" }
    );
    println!("log-level = {}", opts._log_level);
    println!("log-target = {}", opts._log_target);
    println!("exit-idle-time = {}", opts._exit_idle_time);
    println!("scache-idle-time = {}", opts._scache_idle_time);
    println!(
        "disallow-exit = {}",
        if opts._disallow_exit { "yes" } else { "no" }
    );
    println!(
        "disallow-module-loading = {}",
        if opts._disallow_module_loading {
            "yes"
        } else {
            "no"
        }
    );
    println!("default-sample-format = s16le");
    println!("default-sample-rate = {DEFAULT_SAMPLE_RATE}");
    println!("default-sample-channels = {DEFAULT_CHANNELS}");
    println!("default-channel-map = front-left,front-right");
    println!("default-fragments = 4");
    println!("default-fragment-size-msec = 25");
    println!("resample-method = speex-float-1");
    println!("flat-volumes = no");
}

fn dump_available_modules() {
    let modules = [
        ("module-null-sink", "Clocked NULL sink"),
        ("module-null-source", "Clocked NULL source"),
        (
            "module-device-restore",
            "Automatically restore device volumes/mute",
        ),
        (
            "module-stream-restore",
            "Automatically restore stream volumes/mute",
        ),
        ("module-card-restore", "Automatically restore card profiles"),
        ("module-augment-properties", "Augment stream properties"),
        (
            "module-switch-on-port-available",
            "Switch sink/source on port availability",
        ),
        ("module-alsa-card", "ALSA card"),
        ("module-alsa-sink", "ALSA sink"),
        ("module-alsa-source", "ALSA source"),
        (
            "module-native-protocol-unix",
            "Native protocol (UNIX sockets)",
        ),
        ("module-native-protocol-tcp", "Native protocol (TCP)"),
        (
            "module-cli-protocol-unix",
            "Command line interface protocol (UNIX sockets)",
        ),
        (
            "module-default-device-restore",
            "Automatically restore default sink/source",
        ),
        ("module-always-sink", "Always keep at least one sink loaded"),
        (
            "module-rescue-streams",
            "Move streams when sinks/sources are removed",
        ),
        ("module-suspend-on-idle", "Suspend sinks/sources on idle"),
        ("module-loopback", "Loopback from source to sink"),
        ("module-combine-sink", "Combine multiple sinks into one"),
        ("module-remap-sink", "Remap sink channels"),
        ("module-remap-source", "Remap source channels"),
        ("module-echo-cancel", "Echo cancellation"),
        ("module-equalizer-sink", "Equalizer"),
        ("module-ladspa-sink", "LADSPA plugin sink"),
        (
            "module-bluetooth-discover",
            "Bluetooth audio device discovery",
        ),
        ("module-bluetooth-policy", "Bluetooth audio routing policy"),
        ("module-zeroconf-discover", "mDNS/DNS-SD discovery"),
        ("module-zeroconf-publish", "mDNS/DNS-SD publishing"),
        ("module-tunnel-sink", "Tunnel to a remote sink"),
        ("module-tunnel-source", "Tunnel to a remote source"),
        ("module-rtp-send", "RTP sender"),
        ("module-rtp-recv", "RTP receiver"),
        ("module-jack-sink", "JACK sink"),
        ("module-jack-source", "JACK source"),
        ("module-pipe-sink", "UNIX pipe sink"),
        ("module-pipe-source", "UNIX pipe source"),
    ];

    for (name, desc) in modules {
        println!("{name}\n\t{desc}");
    }
}

fn dump_resample_methods() {
    let methods = [
        "src-sinc-best-quality",
        "src-sinc-medium-quality",
        "src-sinc-fastest",
        "src-zero-order-hold",
        "src-linear",
        "trivial",
        "speex-float-0",
        "speex-float-1",
        "speex-float-2",
        "speex-float-3",
        "speex-float-4",
        "speex-float-5",
        "speex-float-6",
        "speex-float-7",
        "speex-float-8",
        "speex-float-9",
        "speex-float-10",
        "speex-fixed-0",
        "speex-fixed-1",
        "speex-fixed-2",
        "speex-fixed-3",
        "speex-fixed-4",
        "speex-fixed-5",
        "speex-fixed-6",
        "speex-fixed-7",
        "speex-fixed-8",
        "speex-fixed-9",
        "speex-fixed-10",
        "ffmpeg",
        "auto",
        "copy",
        "peaks",
        "soxr-mq",
        "soxr-hq",
        "soxr-vhq",
    ];
    for m in methods {
        println!("{m}");
    }
}

fn print_pulseaudio_usage() {
    println!("pulseaudio {VERSION} - PulseAudio sound server");
    println!();
    println!("Usage: pulseaudio [options]");
    println!();
    println!("Options:");
    println!("  -h, --help                  Show this help");
    println!("  --version                   Show version");
    println!("  -D, --daemonize             Daemonize after startup");
    println!("  --system                    Run as system-wide instance");
    println!("  --realtime / --no-realtime  Enable/disable realtime scheduling");
    println!("  --high-priority             Enable high priority scheduling");
    println!("  --disallow-exit             Disallow exit via protocol");
    println!("  --disallow-module-loading   Disallow loading modules after startup");
    println!(
        "  --log-level=LEVEL           Log level (0=error, 1=warn, 2=notice, 3=info, 4=debug)"
    );
    println!("  --log-target=TARGET         Log target (auto, syslog, stderr, file:PATH)");
    println!("  --exit-idle-time=SECS       Exit when idle for N seconds (-1 to disable)");
    println!("  --scache-idle-time=SECS     Unload sample cache entries after N seconds");
    println!("  --check                     Check if daemon is running");
    println!("  -k, --kill                  Kill a running daemon");
    println!("  --start                     Start daemon if not running");
    println!("  --dump-conf                 Dump daemon configuration");
    println!("  --dump-modules              List available modules");
    println!("  --dump-resample-methods     List resample methods");
    println!("  --cleanup-shm               Clean up shared memory");
    println!("  --dl-search-path=PATH       Set module search path");
}

// ---------------------------------------------------------------------------
// Helpers for formatting / output used across personalities
// ---------------------------------------------------------------------------

/// Format a volume as a percentage string (for display).
#[allow(dead_code)]
fn volume_to_percent(vol: u32) -> String {
    let pct = (u64::from(vol) * 100) / u64::from(VOLUME_NORM);
    format!("{pct}%")
}

/// Format a volume as a dB string.
#[allow(dead_code)]
fn volume_to_db(vol: u32) -> String {
    if vol == 0 {
        return "-inf dB".to_string();
    }
    let ratio = vol as f64 / f64::from(VOLUME_NORM);
    let db = 20.0 * ratio.log10();
    format!("{db:.2} dB")
}

/// Validate that a sink index is within bounds.
#[allow(dead_code)]
fn validate_sink_count(count: usize) -> Result<(), String> {
    if count >= MAX_SINKS {
        Err(format!("maximum sink count ({MAX_SINKS}) reached"))
    } else {
        Ok(())
    }
}

/// Validate that a source index is within bounds.
#[allow(dead_code)]
fn validate_source_count(count: usize) -> Result<(), String> {
    if count >= MAX_SOURCES {
        Err(format!("maximum source count ({MAX_SOURCES}) reached"))
    } else {
        Ok(())
    }
}

/// Validate that a card index is within bounds.
#[allow(dead_code)]
fn validate_card_count(count: usize) -> Result<(), String> {
    if count >= MAX_CARDS {
        Err(format!("maximum card count ({MAX_CARDS}) reached"))
    } else {
        Ok(())
    }
}

/// Validate that a sink input index is within bounds.
#[allow(dead_code)]
fn validate_sink_input_count(count: usize) -> Result<(), String> {
    if count >= MAX_SINK_INPUTS {
        Err(format!(
            "maximum sink input count ({MAX_SINK_INPUTS}) reached"
        ))
    } else {
        Ok(())
    }
}

/// Validate that a source output index is within bounds.
#[allow(dead_code)]
fn validate_source_output_count(count: usize) -> Result<(), String> {
    if count >= MAX_SOURCE_OUTPUTS {
        Err(format!(
            "maximum source output count ({MAX_SOURCE_OUTPUTS}) reached"
        ))
    } else {
        Ok(())
    }
}

/// Clamp a volume value to the valid range.
#[allow(dead_code)]
fn clamp_volume(vol: u32) -> u32 {
    vol.min(VOLUME_MAX)
}

/// Convert percentage to raw PA volume.
#[allow(dead_code)]
fn percent_to_volume(pct: u32) -> u32 {
    let v = (u64::from(VOLUME_NORM) * u64::from(pct) / 100) as u32;
    clamp_volume(v)
}

/// Convert raw PA volume to percentage.
#[allow(dead_code)]
fn volume_to_percent_val(vol: u32) -> u32 {
    ((u64::from(vol) * 100) / u64::from(VOLUME_NORM)) as u32
}

/// Check if a string looks like a numeric index.
#[allow(dead_code)]
fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Compute the byte rate for a given spec.
#[allow(dead_code)]
fn compute_byte_rate(format: SampleFormat, channels: u16, rate: u32) -> u64 {
    u64::from(format.bytes_per_sample()) * u64::from(channels) * u64::from(rate)
}

/// Compute the frame size for a given spec.
#[allow(dead_code)]
fn compute_frame_size(format: SampleFormat, channels: u16) -> u32 {
    format.bytes_per_sample() * u32::from(channels)
}

/// Compute the usec duration for a given number of bytes.
#[allow(dead_code)]
fn bytes_to_usec(bytes: u64, spec: &SampleSpec) -> u64 {
    let bps = spec.bytes_per_second();
    if bps == 0 {
        return 0;
    }
    bytes * 1_000_000 / bps
}

/// Compute the number of bytes for a given usec duration.
#[allow(dead_code)]
fn usec_to_bytes(usec: u64, spec: &SampleSpec) -> u64 {
    let bps = spec.bytes_per_second();
    bps * usec / 1_000_000
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("pactl");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let personality = Personality::from_name(&prog_name);
    let sub_args: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match personality {
        Personality::Pactl => run_pactl(&sub_args),
        Personality::Pacmd => run_pacmd(&sub_args),
        Personality::Paplay => run_paplay(&sub_args),
        Personality::Parecord => run_parecord(&sub_args),
        Personality::Pasuspender => run_pasuspender(&sub_args),
        Personality::Pulseaudio => run_pulseaudio(&sub_args),
    };

    process::exit(exit_code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Personality detection -------------------------------------------------

    #[test]
    fn test_personality_from_pactl() {
        assert_eq!(Personality::from_name("pactl"), Personality::Pactl);
    }

    #[test]
    fn test_personality_from_pacmd() {
        assert_eq!(Personality::from_name("pacmd"), Personality::Pacmd);
    }

    #[test]
    fn test_personality_from_paplay() {
        assert_eq!(Personality::from_name("paplay"), Personality::Paplay);
    }

    #[test]
    fn test_personality_from_parecord() {
        assert_eq!(Personality::from_name("parecord"), Personality::Parecord);
    }

    #[test]
    fn test_personality_from_pasuspender() {
        assert_eq!(
            Personality::from_name("pasuspender"),
            Personality::Pasuspender
        );
    }

    #[test]
    fn test_personality_from_pulseaudio() {
        assert_eq!(
            Personality::from_name("pulseaudio"),
            Personality::Pulseaudio
        );
    }

    #[test]
    fn test_personality_unknown_defaults_to_pactl() {
        assert_eq!(Personality::from_name("foobar"), Personality::Pactl);
    }

    #[test]
    fn test_personality_empty_defaults_to_pactl() {
        assert_eq!(Personality::from_name(""), Personality::Pactl);
    }

    #[test]
    fn test_personality_name_round_trip() {
        for p in [
            Personality::Pactl,
            Personality::Pacmd,
            Personality::Paplay,
            Personality::Parecord,
            Personality::Pasuspender,
            Personality::Pulseaudio,
        ] {
            assert_eq!(Personality::from_name(p.name()), p);
        }
    }

    // -- Borrow-safe basename extraction from argv[0] --------------------------

    fn extract_basename(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    }

    #[test]
    fn test_basename_unix_path() {
        assert_eq!(extract_basename("/usr/bin/pactl"), "pactl");
    }

    #[test]
    fn test_basename_windows_path() {
        assert_eq!(extract_basename("C:\\Program Files\\pactl.exe"), "pactl");
    }

    #[test]
    fn test_basename_bare_name() {
        assert_eq!(extract_basename("pacmd"), "pacmd");
    }

    #[test]
    fn test_basename_with_exe() {
        assert_eq!(extract_basename("paplay.exe"), "paplay");
    }

    #[test]
    fn test_basename_nested_unix() {
        assert_eq!(extract_basename("/a/b/c/d/parecord"), "parecord");
    }

    #[test]
    fn test_basename_nested_windows() {
        assert_eq!(
            extract_basename("D:\\a\\b\\c\\pasuspender.exe"),
            "pasuspender"
        );
    }

    #[test]
    fn test_basename_empty() {
        assert_eq!(extract_basename(""), "");
    }

    #[test]
    fn test_basename_trailing_slash() {
        assert_eq!(extract_basename("/usr/bin/"), "");
    }

    // -- SampleFormat ----------------------------------------------------------

    #[test]
    fn test_sample_format_as_str() {
        assert_eq!(SampleFormat::S16Le.as_str(), "s16le");
        assert_eq!(SampleFormat::Float32Le.as_str(), "float32le");
        assert_eq!(SampleFormat::U8.as_str(), "u8");
    }

    #[test]
    fn test_sample_format_bytes_per_sample() {
        assert_eq!(SampleFormat::U8.bytes_per_sample(), 1);
        assert_eq!(SampleFormat::S16Le.bytes_per_sample(), 2);
        assert_eq!(SampleFormat::S24Le.bytes_per_sample(), 3);
        assert_eq!(SampleFormat::Float32Le.bytes_per_sample(), 4);
        assert_eq!(SampleFormat::S32Le.bytes_per_sample(), 4);
    }

    #[test]
    fn test_sample_format_from_str() {
        assert_eq!(
            SampleFormat::from_str_opt("s16le"),
            Some(SampleFormat::S16Le)
        );
        assert_eq!(
            SampleFormat::from_str_opt("float32le"),
            Some(SampleFormat::Float32Le)
        );
        assert_eq!(SampleFormat::from_str_opt("u8"), Some(SampleFormat::U8));
        assert_eq!(SampleFormat::from_str_opt("invalid"), None);
    }

    #[test]
    fn test_sample_format_from_str_alaw_case() {
        assert_eq!(
            SampleFormat::from_str_opt("aLaw"),
            Some(SampleFormat::_Alaw)
        );
        assert_eq!(
            SampleFormat::from_str_opt("alaw"),
            Some(SampleFormat::_Alaw)
        );
    }

    #[test]
    fn test_sample_format_from_str_ulaw_case() {
        assert_eq!(
            SampleFormat::from_str_opt("uLaw"),
            Some(SampleFormat::_Ulaw)
        );
        assert_eq!(
            SampleFormat::from_str_opt("ulaw"),
            Some(SampleFormat::_Ulaw)
        );
    }

    // -- SampleSpec ------------------------------------------------------------

    #[test]
    fn test_sample_spec_bytes_per_second() {
        let spec = SampleSpec::new(SampleFormat::S16Le, 44100, 2);
        assert_eq!(spec.bytes_per_second(), 2 * 44100 * 2);
    }

    #[test]
    fn test_sample_spec_frame_size() {
        let spec = SampleSpec::new(SampleFormat::S16Le, 44100, 2);
        assert_eq!(spec.frame_size(), 4);
    }

    #[test]
    fn test_sample_spec_display() {
        let spec = SampleSpec::new(SampleFormat::S16Le, 44100, 2);
        let s = format!("{spec}");
        assert!(s.contains("s16le"));
        assert!(s.contains("2ch"));
        assert!(s.contains("44100Hz"));
    }

    #[test]
    fn test_sample_spec_mono() {
        let spec = SampleSpec::new(SampleFormat::U8, 8000, 1);
        assert_eq!(spec.bytes_per_second(), 8000);
        assert_eq!(spec.frame_size(), 1);
    }

    #[test]
    fn test_sample_spec_surround() {
        let spec = SampleSpec::new(SampleFormat::Float32Le, 48000, 6);
        assert_eq!(spec.bytes_per_second(), 4 * 48000 * 6);
        assert_eq!(spec.frame_size(), 24);
    }

    // -- ChannelMap ------------------------------------------------------------

    #[test]
    fn test_channel_map_stereo() {
        let map = ChannelMap::stereo();
        assert_eq!(map.positions.len(), 2);
        assert_eq!(map.positions[0], ChannelPosition::FrontLeft);
        assert_eq!(map.positions[1], ChannelPosition::FrontRight);
    }

    #[test]
    fn test_channel_map_mono() {
        let map = ChannelMap::mono();
        assert_eq!(map.positions.len(), 1);
        assert_eq!(map.positions[0], ChannelPosition::Mono);
    }

    #[test]
    fn test_channel_map_surround51() {
        let map = ChannelMap::surround51();
        assert_eq!(map.positions.len(), 6);
    }

    #[test]
    fn test_channel_map_display() {
        let map = ChannelMap::stereo();
        let s = format!("{map}");
        assert_eq!(s, "front-left,front-right");
    }

    #[test]
    fn test_channel_map_mono_display() {
        let map = ChannelMap::mono();
        let s = format!("{map}");
        assert_eq!(s, "mono");
    }

    // -- Volume ----------------------------------------------------------------

    #[test]
    fn test_volume_new() {
        let v = Volume::new(2, 65536);
        assert_eq!(v.values.len(), 2);
        assert_eq!(v.values[0], 65536);
        assert_eq!(v.values[1], 65536);
    }

    #[test]
    fn test_volume_average() {
        let v = Volume::new(2, 65536);
        assert_eq!(v.average(), 65536);
    }

    #[test]
    fn test_volume_average_mixed() {
        let v = Volume {
            values: vec![0, 65536],
        };
        assert_eq!(v.average(), 32768);
    }

    #[test]
    fn test_volume_average_empty() {
        let v = Volume { values: Vec::new() };
        assert_eq!(v.average(), 0);
    }

    #[test]
    fn test_volume_set_all() {
        let mut v = Volume::new(2, 65536);
        v.set_all(32768);
        assert_eq!(v.values[0], 32768);
        assert_eq!(v.values[1], 32768);
    }

    #[test]
    fn test_volume_percent_str() {
        let v = Volume::new(2, 65536);
        assert_eq!(v.percent_str(), "100% 100%");
    }

    #[test]
    fn test_volume_percent_str_half() {
        let v = Volume::new(1, 32768);
        assert_eq!(v.percent_str(), "50%");
    }

    #[test]
    fn test_volume_percent_str_zero() {
        let v = Volume::new(1, 0);
        assert_eq!(v.percent_str(), "0%");
    }

    #[test]
    fn test_volume_db_str_silence() {
        let v = Volume::new(1, 0);
        assert_eq!(v.db_str(), "-inf dB");
    }

    #[test]
    fn test_volume_db_str_full() {
        let v = Volume::new(1, VOLUME_NORM);
        let s = v.db_str();
        assert!(s.contains("0.00 dB"));
    }

    #[test]
    fn test_volume_raw_str() {
        let v = Volume::new(2, 65536);
        assert_eq!(v.raw_str(), "65536 65536");
    }

    // -- Volume parsing --------------------------------------------------------

    #[test]
    fn test_parse_volume_absolute() {
        assert_eq!(parse_volume("32768", 0).unwrap(), 32768);
    }

    #[test]
    fn test_parse_volume_percent() {
        assert_eq!(parse_volume("50%", 0).unwrap(), 32768);
    }

    #[test]
    fn test_parse_volume_percent_100() {
        assert_eq!(parse_volume("100%", 0).unwrap(), 65536);
    }

    #[test]
    fn test_parse_volume_relative_plus_percent() {
        // +10% on a current of 65536 (100%)
        let result = parse_volume("+10%", 65536).unwrap();
        let expected = 65536 + 6553;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_volume_relative_minus_percent() {
        let result = parse_volume("-10%", 65536).unwrap();
        let expected = 65536 - 6553;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_volume_0db() {
        let result = parse_volume("0dB", 0).unwrap();
        assert_eq!(result, VOLUME_NORM);
    }

    #[test]
    fn test_parse_volume_clamped() {
        let result = parse_volume("200%", 0).unwrap();
        assert!(result <= VOLUME_MAX);
    }

    #[test]
    fn test_parse_volume_empty() {
        assert!(parse_volume("", 0).is_err());
    }

    #[test]
    fn test_parse_volume_invalid() {
        assert!(parse_volume("abc", 0).is_err());
    }

    #[test]
    fn test_parse_volume_relative_plus_raw() {
        let result = parse_volume("+100", 1000).unwrap();
        assert_eq!(result, 1100);
    }

    #[test]
    fn test_parse_volume_relative_minus_raw() {
        let result = parse_volume("-100", 1000).unwrap();
        assert_eq!(result, 900);
    }

    #[test]
    fn test_parse_volume_minus_saturate() {
        let result = parse_volume("-100", 50).unwrap();
        assert_eq!(result, 0);
    }

    // -- Mute parsing ----------------------------------------------------------

    #[test]
    fn test_parse_mute_true() {
        assert!(parse_mute("1", false).unwrap());
        assert!(parse_mute("true", false).unwrap());
        assert!(parse_mute("yes", false).unwrap());
    }

    #[test]
    fn test_parse_mute_false() {
        assert!(!parse_mute("0", true).unwrap());
        assert!(!parse_mute("false", true).unwrap());
        assert!(!parse_mute("no", true).unwrap());
    }

    #[test]
    fn test_parse_mute_toggle() {
        assert!(parse_mute("toggle", false).unwrap());
        assert!(!parse_mute("toggle", true).unwrap());
    }

    #[test]
    fn test_parse_mute_invalid() {
        assert!(parse_mute("maybe", false).is_err());
    }

    #[test]
    fn test_parse_mute_case_insensitive() {
        assert!(parse_mute("TRUE", false).unwrap());
        assert!(parse_mute("Yes", false).unwrap());
        assert!(!parse_mute("FALSE", true).unwrap());
        assert!(!parse_mute("No", true).unwrap());
        assert!(parse_mute("TOGGLE", false).unwrap());
    }

    // -- PactlCommand parsing --------------------------------------------------

    #[test]
    fn test_pactl_command_from_str() {
        assert_eq!(PactlCommand::from_str_opt("list"), Some(PactlCommand::List));
        assert_eq!(PactlCommand::from_str_opt("info"), Some(PactlCommand::Info));
        assert_eq!(PactlCommand::from_str_opt("stat"), Some(PactlCommand::Stat));
        assert_eq!(
            PactlCommand::from_str_opt("set-sink-volume"),
            Some(PactlCommand::SetSinkVolume)
        );
        assert_eq!(
            PactlCommand::from_str_opt("set-source-volume"),
            Some(PactlCommand::SetSourceVolume)
        );
        assert_eq!(
            PactlCommand::from_str_opt("set-sink-mute"),
            Some(PactlCommand::SetSinkMute)
        );
        assert_eq!(
            PactlCommand::from_str_opt("set-source-mute"),
            Some(PactlCommand::SetSourceMute)
        );
    }

    #[test]
    fn test_pactl_command_from_str_more() {
        assert_eq!(
            PactlCommand::from_str_opt("set-default-sink"),
            Some(PactlCommand::SetDefaultSink)
        );
        assert_eq!(
            PactlCommand::from_str_opt("set-default-source"),
            Some(PactlCommand::SetDefaultSource)
        );
        assert_eq!(
            PactlCommand::from_str_opt("move-sink-input"),
            Some(PactlCommand::MoveSinkInput)
        );
        assert_eq!(
            PactlCommand::from_str_opt("move-source-output"),
            Some(PactlCommand::MoveSourceOutput)
        );
        assert_eq!(
            PactlCommand::from_str_opt("load-module"),
            Some(PactlCommand::LoadModule)
        );
        assert_eq!(
            PactlCommand::from_str_opt("unload-module"),
            Some(PactlCommand::UnloadModule)
        );
        assert_eq!(
            PactlCommand::from_str_opt("set-card-profile"),
            Some(PactlCommand::SetCardProfile)
        );
        assert_eq!(
            PactlCommand::from_str_opt("subscribe"),
            Some(PactlCommand::Subscribe)
        );
        assert_eq!(PactlCommand::from_str_opt("exit"), Some(PactlCommand::Exit));
    }

    #[test]
    fn test_pactl_command_unknown() {
        assert_eq!(PactlCommand::from_str_opt("foobar"), None);
    }

    // -- ListEntity parsing ----------------------------------------------------

    #[test]
    fn test_list_entity_from_str() {
        assert_eq!(ListEntity::from_str_opt("sinks"), Some(ListEntity::Sinks));
        assert_eq!(
            ListEntity::from_str_opt("sources"),
            Some(ListEntity::Sources)
        );
        assert_eq!(
            ListEntity::from_str_opt("sink-inputs"),
            Some(ListEntity::SinkInputs)
        );
        assert_eq!(
            ListEntity::from_str_opt("source-outputs"),
            Some(ListEntity::SourceOutputs)
        );
        assert_eq!(ListEntity::from_str_opt("cards"), Some(ListEntity::Cards));
        assert_eq!(
            ListEntity::from_str_opt("modules"),
            Some(ListEntity::Modules)
        );
        assert_eq!(
            ListEntity::from_str_opt("clients"),
            Some(ListEntity::Clients)
        );
    }

    #[test]
    fn test_list_entity_case_insensitive() {
        assert_eq!(ListEntity::from_str_opt("SINKS"), Some(ListEntity::Sinks));
        assert_eq!(
            ListEntity::from_str_opt("Sources"),
            Some(ListEntity::Sources)
        );
    }

    #[test]
    fn test_list_entity_unknown() {
        assert_eq!(ListEntity::from_str_opt("foobar"), None);
    }

    // -- Default state ---------------------------------------------------------

    #[test]
    fn test_default_state_has_sinks() {
        let state = PulseState::default_state();
        assert_eq!(state.sinks.len(), 2);
    }

    #[test]
    fn test_default_state_has_sources() {
        let state = PulseState::default_state();
        assert_eq!(state.sources.len(), 2);
    }

    #[test]
    fn test_default_state_has_cards() {
        let state = PulseState::default_state();
        assert_eq!(state.cards.len(), 1);
    }

    #[test]
    fn test_default_state_has_modules() {
        let state = PulseState::default_state();
        assert_eq!(state.modules.len(), 10);
    }

    #[test]
    fn test_default_state_has_clients() {
        let state = PulseState::default_state();
        assert_eq!(state.clients.len(), 1);
    }

    #[test]
    fn test_default_state_has_sink_inputs() {
        let state = PulseState::default_state();
        assert_eq!(state.sink_inputs.len(), 1);
    }

    #[test]
    fn test_default_state_default_sink_name() {
        let state = PulseState::default_state();
        assert_eq!(
            state.server.default_sink_name,
            "alsa_output.pci-0000_00_1f.3.analog-stereo"
        );
    }

    #[test]
    fn test_default_state_default_source_name() {
        let state = PulseState::default_state();
        assert_eq!(
            state.server.default_source_name,
            "alsa_input.pci-0000_00_1f.3.analog-stereo"
        );
    }

    // -- Sink/source/card lookup -----------------------------------------------

    #[test]
    fn test_find_sink_by_index() {
        let state = PulseState::default_state();
        assert_eq!(state.find_sink_by_name_or_index("0"), Some(0));
        assert_eq!(state.find_sink_by_name_or_index("1"), Some(1));
    }

    #[test]
    fn test_find_sink_by_name() {
        let state = PulseState::default_state();
        assert_eq!(
            state.find_sink_by_name_or_index("alsa_output.pci-0000_00_1f.3.analog-stereo"),
            Some(0)
        );
    }

    #[test]
    fn test_find_sink_not_found() {
        let state = PulseState::default_state();
        assert_eq!(state.find_sink_by_name_or_index("99"), None);
        assert_eq!(state.find_sink_by_name_or_index("nosuchsink"), None);
    }

    #[test]
    fn test_find_source_by_index() {
        let state = PulseState::default_state();
        assert_eq!(state.find_source_by_name_or_index("0"), Some(0));
        assert_eq!(state.find_source_by_name_or_index("1"), Some(1));
    }

    #[test]
    fn test_find_source_by_name() {
        let state = PulseState::default_state();
        assert_eq!(
            state.find_source_by_name_or_index("alsa_input.pci-0000_00_1f.3.analog-stereo"),
            Some(0)
        );
    }

    #[test]
    fn test_find_source_not_found() {
        let state = PulseState::default_state();
        assert_eq!(state.find_source_by_name_or_index("99"), None);
    }

    #[test]
    fn test_find_card_by_index() {
        let state = PulseState::default_state();
        assert_eq!(state.find_card_by_name_or_index("0"), Some(0));
    }

    #[test]
    fn test_find_card_by_name() {
        let state = PulseState::default_state();
        assert_eq!(
            state.find_card_by_name_or_index("alsa_card.pci-0000_00_1f.3"),
            Some(0)
        );
    }

    #[test]
    fn test_find_card_not_found() {
        let state = PulseState::default_state();
        assert_eq!(state.find_card_by_name_or_index("99"), None);
    }

    #[test]
    fn test_find_module_by_index() {
        let state = PulseState::default_state();
        assert_eq!(state.find_module_by_index(0), Some(0));
        assert_eq!(state.find_module_by_index(9), Some(9));
        assert_eq!(state.find_module_by_index(99), None);
    }

    #[test]
    fn test_find_sink_input_by_index() {
        let state = PulseState::default_state();
        assert_eq!(state.find_sink_input_by_index(0), Some(0));
        assert_eq!(state.find_sink_input_by_index(99), None);
    }

    #[test]
    fn test_find_source_output_by_index() {
        let state = PulseState::default_state();
        assert_eq!(state.find_source_output_by_index(0), None);
    }

    // -- pactl set-sink-volume -------------------------------------------------

    #[test]
    fn test_set_sink_volume_by_index() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string(), "32768".to_string()];
        let rc = run_pactl_set_sink_volume(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(state.sinks[0].volume.average(), 32768);
    }

    #[test]
    fn test_set_sink_volume_by_name() {
        let mut state = PulseState::default_state();
        let args = vec![
            "alsa_output.pci-0000_00_1f.3.analog-stereo".to_string(),
            "50%".to_string(),
        ];
        let rc = run_pactl_set_sink_volume(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(state.sinks[0].volume.average(), 32768);
    }

    #[test]
    fn test_set_sink_volume_not_found() {
        let mut state = PulseState::default_state();
        let args = vec!["nosuchsink".to_string(), "50%".to_string()];
        let rc = run_pactl_set_sink_volume(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_set_sink_volume_missing_args() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string()];
        let rc = run_pactl_set_sink_volume(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl set-source-volume -----------------------------------------------

    #[test]
    fn test_set_source_volume_by_index() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string(), "32768".to_string()];
        let rc = run_pactl_set_source_volume(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(state.sources[0].volume.average(), 32768);
    }

    #[test]
    fn test_set_source_volume_not_found() {
        let mut state = PulseState::default_state();
        let args = vec!["nosuch".to_string(), "50%".to_string()];
        let rc = run_pactl_set_source_volume(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl set-sink-mute ---------------------------------------------------

    #[test]
    fn test_set_sink_mute_on() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string(), "1".to_string()];
        let rc = run_pactl_set_sink_mute(&args, &mut state);
        assert_eq!(rc, 0);
        assert!(state.sinks[0].muted);
    }

    #[test]
    fn test_set_sink_mute_off() {
        let mut state = PulseState::default_state();
        state.sinks[0].muted = true;
        let args = vec!["0".to_string(), "0".to_string()];
        let rc = run_pactl_set_sink_mute(&args, &mut state);
        assert_eq!(rc, 0);
        assert!(!state.sinks[0].muted);
    }

    #[test]
    fn test_set_sink_mute_toggle() {
        let mut state = PulseState::default_state();
        assert!(!state.sinks[0].muted);
        let args = vec!["0".to_string(), "toggle".to_string()];
        let rc = run_pactl_set_sink_mute(&args, &mut state);
        assert_eq!(rc, 0);
        assert!(state.sinks[0].muted);
    }

    #[test]
    fn test_set_sink_mute_not_found() {
        let mut state = PulseState::default_state();
        let args = vec!["99".to_string(), "1".to_string()];
        let rc = run_pactl_set_sink_mute(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl set-source-mute -------------------------------------------------

    #[test]
    fn test_set_source_mute_on() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string(), "1".to_string()];
        let rc = run_pactl_set_source_mute(&args, &mut state);
        assert_eq!(rc, 0);
        assert!(state.sources[0].muted);
    }

    #[test]
    fn test_set_source_mute_toggle() {
        let mut state = PulseState::default_state();
        assert!(!state.sources[0].muted);
        let args = vec!["0".to_string(), "toggle".to_string()];
        let rc = run_pactl_set_source_mute(&args, &mut state);
        assert_eq!(rc, 0);
        assert!(state.sources[0].muted);
    }

    // -- pactl set-default-sink ------------------------------------------------

    #[test]
    fn test_set_default_sink_by_index() {
        let mut state = PulseState::default_state();
        let args = vec!["1".to_string()];
        let rc = run_pactl_set_default_sink(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(
            state.server.default_sink_name,
            "alsa_output.pci-0000_01_00.1.hdmi-stereo"
        );
    }

    #[test]
    fn test_set_default_sink_by_name() {
        let mut state = PulseState::default_state();
        let args = vec!["alsa_output.pci-0000_01_00.1.hdmi-stereo".to_string()];
        let rc = run_pactl_set_default_sink(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(
            state.server.default_sink_name,
            "alsa_output.pci-0000_01_00.1.hdmi-stereo"
        );
    }

    #[test]
    fn test_set_default_sink_not_found() {
        let mut state = PulseState::default_state();
        let args = vec!["nosink".to_string()];
        let rc = run_pactl_set_default_sink(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_set_default_sink_no_args() {
        let mut state = PulseState::default_state();
        let args: Vec<String> = Vec::new();
        let rc = run_pactl_set_default_sink(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl set-default-source ----------------------------------------------

    #[test]
    fn test_set_default_source_by_index() {
        let mut state = PulseState::default_state();
        let args = vec!["1".to_string()];
        let rc = run_pactl_set_default_source(&args, &mut state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_set_default_source_not_found() {
        let mut state = PulseState::default_state();
        let args = vec!["nosource".to_string()];
        let rc = run_pactl_set_default_source(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl move-sink-input -------------------------------------------------

    #[test]
    fn test_move_sink_input() {
        let mut state = PulseState::default_state();
        assert_eq!(state.sink_inputs[0].sink, 0);
        let args = vec!["0".to_string(), "1".to_string()];
        let rc = run_pactl_move_sink_input(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(state.sink_inputs[0].sink, 1);
    }

    #[test]
    fn test_move_sink_input_bad_index() {
        let mut state = PulseState::default_state();
        let args = vec!["99".to_string(), "0".to_string()];
        let rc = run_pactl_move_sink_input(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_move_sink_input_bad_sink() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string(), "99".to_string()];
        let rc = run_pactl_move_sink_input(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_move_sink_input_not_numeric() {
        let mut state = PulseState::default_state();
        let args = vec!["abc".to_string(), "0".to_string()];
        let rc = run_pactl_move_sink_input(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl move-source-output ----------------------------------------------

    #[test]
    fn test_move_source_output_bad_index() {
        let mut state = PulseState::default_state();
        let args = vec!["99".to_string(), "0".to_string()];
        let rc = run_pactl_move_source_output(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_move_source_output_not_numeric() {
        let mut state = PulseState::default_state();
        let args = vec!["abc".to_string(), "0".to_string()];
        let rc = run_pactl_move_source_output(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl load-module / unload-module ------------------------------------

    #[test]
    fn test_load_module() {
        let mut state = PulseState::default_state();
        let initial = state.modules.len();
        let args = vec!["module-null-sink".to_string()];
        let rc = run_pactl_load_module(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(state.modules.len(), initial + 1);
    }

    #[test]
    fn test_load_module_with_args() {
        let mut state = PulseState::default_state();
        let args = vec![
            "module-null-sink".to_string(),
            "sink_name=test".to_string(),
            "rate=48000".to_string(),
        ];
        let rc = run_pactl_load_module(&args, &mut state);
        assert_eq!(rc, 0);
        let last = state.modules.last().unwrap();
        assert_eq!(last.name, "module-null-sink");
        assert_eq!(last.argument, "sink_name=test rate=48000");
    }

    #[test]
    fn test_load_module_no_name() {
        let mut state = PulseState::default_state();
        let args: Vec<String> = Vec::new();
        let rc = run_pactl_load_module(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_unload_module() {
        let mut state = PulseState::default_state();
        let initial = state.modules.len();
        let args = vec!["0".to_string()];
        let rc = run_pactl_unload_module(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(state.modules.len(), initial - 1);
    }

    #[test]
    fn test_unload_module_not_found() {
        let mut state = PulseState::default_state();
        let args = vec!["999".to_string()];
        let rc = run_pactl_unload_module(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_unload_module_invalid_index() {
        let mut state = PulseState::default_state();
        let args = vec!["abc".to_string()];
        let rc = run_pactl_unload_module(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- pactl set-card-profile ------------------------------------------------

    #[test]
    fn test_set_card_profile() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string(), "off".to_string()];
        let rc = run_pactl_set_card_profile(&args, &mut state);
        assert_eq!(rc, 0);
        assert_eq!(state.cards[0].active_profile, "off");
    }

    #[test]
    fn test_set_card_profile_not_found() {
        let mut state = PulseState::default_state();
        let args = vec!["99".to_string(), "off".to_string()];
        let rc = run_pactl_set_card_profile(&args, &mut state);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_set_card_profile_invalid_profile() {
        let mut state = PulseState::default_state();
        let args = vec!["0".to_string(), "nosuchprofile".to_string()];
        let rc = run_pactl_set_card_profile(&args, &mut state);
        assert_eq!(rc, 1);
    }

    // -- Format info output ----------------------------------------------------

    #[test]
    fn test_sink_format_info_contains_name() {
        let state = PulseState::default_state();
        let info = state.sinks[0].format_info();
        assert!(info.contains("alsa_output.pci-0000_00_1f.3.analog-stereo"));
    }

    #[test]
    fn test_source_format_info_contains_name() {
        let state = PulseState::default_state();
        let info = state.sources[0].format_info();
        assert!(info.contains("alsa_input.pci-0000_00_1f.3.analog-stereo"));
    }

    #[test]
    fn test_card_format_info_contains_profiles() {
        let state = PulseState::default_state();
        let info = state.cards[0].format_info();
        assert!(info.contains("Analog Stereo Duplex"));
        assert!(info.contains("Off"));
    }

    #[test]
    fn test_module_format_info_contains_name() {
        let state = PulseState::default_state();
        let info = state.modules[0].format_info();
        assert!(info.contains("module-device-restore"));
    }

    #[test]
    fn test_client_format_info_contains_name() {
        let state = PulseState::default_state();
        let info = state.clients[0].format_info();
        assert!(info.contains("PulseAudio Control"));
    }

    #[test]
    fn test_sink_input_format_info_contains_name() {
        let state = PulseState::default_state();
        let info = state.sink_inputs[0].format_info();
        assert!(info.contains("Playback Stream"));
    }

    #[test]
    fn test_server_info_format() {
        let state = PulseState::default_state();
        let info = state.server.format_info();
        assert!(info.contains("pulseaudio"));
        assert!(info.contains("Default Sink"));
    }

    #[test]
    fn test_stat_info_format() {
        let state = PulseState::default_state();
        let info = state.stat.format_info();
        assert!(info.contains("Currently in use"));
        assert!(info.contains("Sample cache size"));
    }

    // -- Helper function tests -------------------------------------------------

    #[test]
    fn test_volume_to_percent_100() {
        assert_eq!(volume_to_percent(VOLUME_NORM), "100%");
    }

    #[test]
    fn test_volume_to_percent_0() {
        assert_eq!(volume_to_percent(0), "0%");
    }

    #[test]
    fn test_volume_to_percent_50() {
        assert_eq!(volume_to_percent(32768), "50%");
    }

    #[test]
    fn test_volume_to_db_zero() {
        assert_eq!(volume_to_db(0), "-inf dB");
    }

    #[test]
    fn test_volume_to_db_100() {
        let s = volume_to_db(VOLUME_NORM);
        assert!(s.contains("0.00 dB"));
    }

    #[test]
    fn test_validate_sink_count_ok() {
        assert!(validate_sink_count(0).is_ok());
        assert!(validate_sink_count(MAX_SINKS - 1).is_ok());
    }

    #[test]
    fn test_validate_sink_count_full() {
        assert!(validate_sink_count(MAX_SINKS).is_err());
    }

    #[test]
    fn test_validate_source_count_ok() {
        assert!(validate_source_count(0).is_ok());
    }

    #[test]
    fn test_validate_source_count_full() {
        assert!(validate_source_count(MAX_SOURCES).is_err());
    }

    #[test]
    fn test_validate_card_count_ok() {
        assert!(validate_card_count(0).is_ok());
    }

    #[test]
    fn test_validate_card_count_full() {
        assert!(validate_card_count(MAX_CARDS).is_err());
    }

    #[test]
    fn test_validate_sink_input_count_ok() {
        assert!(validate_sink_input_count(0).is_ok());
    }

    #[test]
    fn test_validate_sink_input_count_full() {
        assert!(validate_sink_input_count(MAX_SINK_INPUTS).is_err());
    }

    #[test]
    fn test_validate_source_output_count_ok() {
        assert!(validate_source_output_count(0).is_ok());
    }

    #[test]
    fn test_validate_source_output_count_full() {
        assert!(validate_source_output_count(MAX_SOURCE_OUTPUTS).is_err());
    }

    #[test]
    fn test_clamp_volume_normal() {
        assert_eq!(clamp_volume(1000), 1000);
    }

    #[test]
    fn test_clamp_volume_over() {
        assert_eq!(clamp_volume(999_999), VOLUME_MAX);
    }

    #[test]
    fn test_percent_to_volume_100() {
        assert_eq!(percent_to_volume(100), VOLUME_NORM);
    }

    #[test]
    fn test_percent_to_volume_0() {
        assert_eq!(percent_to_volume(0), 0);
    }

    #[test]
    fn test_percent_to_volume_50() {
        assert_eq!(percent_to_volume(50), 32768);
    }

    #[test]
    fn test_percent_to_volume_150() {
        assert_eq!(percent_to_volume(150), VOLUME_MAX);
    }

    #[test]
    fn test_volume_to_percent_val() {
        assert_eq!(volume_to_percent_val(VOLUME_NORM), 100);
        assert_eq!(volume_to_percent_val(32768), 50);
        assert_eq!(volume_to_percent_val(0), 0);
    }

    #[test]
    fn test_is_numeric() {
        assert!(is_numeric("123"));
        assert!(is_numeric("0"));
        assert!(!is_numeric(""));
        assert!(!is_numeric("abc"));
        assert!(!is_numeric("12a"));
    }

    #[test]
    fn test_compute_byte_rate() {
        assert_eq!(compute_byte_rate(SampleFormat::S16Le, 2, 44100), 176400);
    }

    #[test]
    fn test_compute_frame_size() {
        assert_eq!(compute_frame_size(SampleFormat::S16Le, 2), 4);
        assert_eq!(compute_frame_size(SampleFormat::Float32Le, 6), 24);
    }

    #[test]
    fn test_bytes_to_usec() {
        let spec = SampleSpec::new(SampleFormat::S16Le, 44100, 2);
        // 176400 bytes/sec -> 1 second = 1_000_000 usec
        let usec = bytes_to_usec(176400, &spec);
        assert_eq!(usec, 1_000_000);
    }

    #[test]
    fn test_bytes_to_usec_zero_rate() {
        let spec = SampleSpec::new(SampleFormat::S16Le, 0, 2);
        assert_eq!(bytes_to_usec(100, &spec), 0);
    }

    #[test]
    fn test_usec_to_bytes() {
        let spec = SampleSpec::new(SampleFormat::S16Le, 44100, 2);
        let bytes = usec_to_bytes(1_000_000, &spec);
        assert_eq!(bytes, 176400);
    }

    // -- Port / PortAvailability -----------------------------------------------

    #[test]
    fn test_port_availability_as_str() {
        assert_eq!(PortAvailability::Unknown.as_str(), "unknown");
        assert_eq!(PortAvailability::Available.as_str(), "available");
        assert_eq!(PortAvailability::_Unavailable.as_str(), "not available");
    }

    #[test]
    fn test_port_new() {
        let p = Port::new("test-port", "Test Port", 100, PortAvailability::Available);
        assert_eq!(p.name, "test-port");
        assert_eq!(p.description, "Test Port");
        assert_eq!(p.priority, 100);
        assert_eq!(p.availability, PortAvailability::Available);
    }

    // -- SinkState / SourceState -----------------------------------------------

    #[test]
    fn test_sink_state_as_str() {
        assert_eq!(SinkState::Running.as_str(), "RUNNING");
        assert_eq!(SinkState::Idle.as_str(), "IDLE");
        assert_eq!(SinkState::Suspended.as_str(), "SUSPENDED");
    }

    #[test]
    fn test_source_state_as_str() {
        assert_eq!(SourceState::Running.as_str(), "RUNNING");
        assert_eq!(SourceState::Idle.as_str(), "IDLE");
        assert_eq!(SourceState::Suspended.as_str(), "SUSPENDED");
    }

    // -- ChannelPosition -------------------------------------------------------

    #[test]
    fn test_channel_position_as_str() {
        assert_eq!(ChannelPosition::Mono.as_str(), "mono");
        assert_eq!(ChannelPosition::FrontLeft.as_str(), "front-left");
        assert_eq!(ChannelPosition::FrontRight.as_str(), "front-right");
        assert_eq!(ChannelPosition::FrontCenter.as_str(), "front-center");
        assert_eq!(ChannelPosition::RearLeft.as_str(), "rear-left");
        assert_eq!(ChannelPosition::RearRight.as_str(), "rear-right");
        assert_eq!(ChannelPosition::Lfe.as_str(), "lfe");
    }

    // -- CardProfile -----------------------------------------------------------

    #[test]
    fn test_card_profile_format_info() {
        let p = CardProfile::new("output:stereo", "Stereo Output", 1, 0, 5000);
        let info = p.format_info();
        assert!(info.contains("output:stereo"));
        assert!(info.contains("Stereo Output"));
        assert!(info.contains("sinks: 1"));
        assert!(info.contains("sources: 0"));
    }

    // -- pactl help/version paths ----------------------------------------------

    #[test]
    fn test_pactl_help_flag() {
        let rc = run_pactl(&["--help".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_version_flag() {
        let rc = run_pactl(&["--version".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_no_args() {
        let rc = run_pactl(&[]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_pactl_unknown_command() {
        let rc = run_pactl(&["bogus".to_string()]);
        assert_eq!(rc, 1);
    }

    // -- pactl subscribe / exit ------------------------------------------------

    #[test]
    fn test_pactl_subscribe() {
        let rc = run_pactl_subscribe();
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_exit() {
        let rc = run_pactl_exit();
        assert_eq!(rc, 0);
    }

    // -- pactl info / stat -----------------------------------------------------

    #[test]
    fn test_pactl_info() {
        let state = PulseState::default_state();
        let rc = run_pactl_info(&state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_stat() {
        let state = PulseState::default_state();
        let rc = run_pactl_stat(&state);
        assert_eq!(rc, 0);
    }

    // -- pacmd dispatch --------------------------------------------------------

    #[test]
    fn test_pacmd_help() {
        assert_eq!(run_pacmd(&["help".to_string()]), 0);
    }

    #[test]
    fn test_pacmd_version() {
        let _ = run_pacmd(&["--version".to_string()]);
    }

    #[test]
    fn test_pacmd_no_args() {
        let _ = run_pacmd(&[]);
    }

    #[test]
    fn test_pacmd_unknown_command() {
        assert_eq!(run_pacmd(&["bogus".to_string()]), 1);
    }

    #[test]
    fn test_pacmd_list_sinks() {
        assert_eq!(run_pacmd(&["list-sinks".to_string()]), 0);
    }

    #[test]
    fn test_pacmd_list_sources() {
        assert_eq!(run_pacmd(&["list-sources".to_string()]), 0);
    }

    #[test]
    fn test_pacmd_list_modules() {
        assert_eq!(run_pacmd(&["list-modules".to_string()]), 0);
    }

    #[test]
    fn test_pacmd_stat() {
        assert_eq!(run_pacmd(&["stat".to_string()]), 0);
    }

    #[test]
    fn test_pacmd_dump() {
        assert_eq!(run_pacmd(&["dump".to_string()]), 0);
    }

    #[test]
    fn test_pacmd_exit() {
        assert_eq!(run_pacmd(&["exit".to_string()]), 0);
    }

    // -- paplay ----------------------------------------------------------------

    #[test]
    fn test_paplay_help() {
        assert_eq!(run_paplay(&["--help".to_string()]), 0);
    }

    #[test]
    fn test_paplay_version() {
        let _ = run_paplay(&["--version".to_string()]);
    }

    #[test]
    fn test_paplay_no_file() {
        assert_eq!(run_paplay(&[]), 1);
    }

    #[test]
    fn test_paplay_file() {
        assert_eq!(run_paplay(&["test.wav".to_string()]), 0);
    }

    #[test]
    fn test_paplay_with_device() {
        assert_eq!(
            run_paplay(&[
                "-d".to_string(),
                "mysink".to_string(),
                "test.wav".to_string(),
            ]),
            0
        );
    }

    #[test]
    fn test_paplay_list_file_formats() {
        assert_eq!(run_paplay(&["--list-file-formats".to_string()]), 0);
    }

    #[test]
    fn test_paplay_unknown_option() {
        assert_eq!(run_paplay(&["--bogus".to_string()]), 1);
    }

    // -- parecord --------------------------------------------------------------

    #[test]
    fn test_parecord_help() {
        assert_eq!(run_parecord(&["--help".to_string()]), 0);
    }

    #[test]
    fn test_parecord_version() {
        let _ = run_parecord(&["--version".to_string()]);
    }

    #[test]
    fn test_parecord_no_file() {
        assert_eq!(run_parecord(&[]), 1);
    }

    #[test]
    fn test_parecord_file() {
        assert_eq!(run_parecord(&["output.wav".to_string()]), 0);
    }

    #[test]
    fn test_parecord_with_device() {
        assert_eq!(
            run_parecord(&[
                "-d".to_string(),
                "mysource".to_string(),
                "output.wav".to_string(),
            ]),
            0
        );
    }

    #[test]
    fn test_parecord_unknown_option() {
        assert_eq!(run_parecord(&["--bogus".to_string()]), 1);
    }

    // -- pasuspender -----------------------------------------------------------

    #[test]
    fn test_pasuspender_help() {
        assert_eq!(run_pasuspender(&["--help".to_string()]), 0);
    }

    #[test]
    fn test_pasuspender_version() {
        let _ = run_pasuspender(&["--version".to_string()]);
    }

    #[test]
    fn test_pasuspender_no_command() {
        assert_eq!(run_pasuspender(&[]), 1);
    }

    #[test]
    fn test_pasuspender_with_command() {
        assert_eq!(
            run_pasuspender(&["--".to_string(), "aplay".to_string()]),
            0
        );
    }

    #[test]
    fn test_pasuspender_with_server() {
        assert_eq!(
            run_pasuspender(&[
                "-s".to_string(),
                "localhost".to_string(),
                "--".to_string(),
                "cmd".to_string(),
            ]),
            0
        );
    }

    // -- pulseaudio daemon -----------------------------------------------------

    #[test]
    fn test_pulseaudio_help() {
        assert_eq!(run_pulseaudio(&["--help".to_string()]), 0);
    }

    #[test]
    fn test_pulseaudio_version() {
        let _ = run_pulseaudio(&["--version".to_string()]);
    }

    #[test]
    fn test_pulseaudio_check() {
        assert_eq!(run_pulseaudio(&["--check".to_string()]), 0);
    }

    #[test]
    fn test_pulseaudio_kill() {
        assert_eq!(run_pulseaudio(&["-k".to_string()]), 0);
    }

    #[test]
    fn test_pulseaudio_start() {
        assert_eq!(run_pulseaudio(&["--start".to_string()]), 0);
    }

    #[test]
    fn test_pulseaudio_dump_conf() {
        assert_eq!(run_pulseaudio(&["--dump-conf".to_string()]), 0);
    }

    #[test]
    fn test_pulseaudio_dump_modules() {
        assert_eq!(run_pulseaudio(&["--dump-modules".to_string()]), 0);
    }

    #[test]
    fn test_pulseaudio_dump_resample_methods() {
        assert_eq!(
            run_pulseaudio(&["--dump-resample-methods".to_string()]),
            0
        );
    }

    #[test]
    fn test_pulseaudio_cleanup_shm() {
        assert_eq!(run_pulseaudio(&["--cleanup-shm".to_string()]), 0);
    }

    #[test]
    fn test_pulseaudio_default() {
        let _ = run_pulseaudio(&[]);
    }

    #[test]
    fn test_pulseaudio_unknown_option() {
        assert_eq!(run_pulseaudio(&["--bogus".to_string()]), 1);
    }

    // -- pactl list variations -------------------------------------------------

    #[test]
    fn test_pactl_list_all() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&[], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_sinks() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["sinks".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_sources() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["sources".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_sink_inputs() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["sink-inputs".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_source_outputs() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["source-outputs".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_cards() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["cards".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_modules() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["modules".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_clients() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["clients".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_short() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["sinks".to_string(), "short".to_string()], &state);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_pactl_list_invalid_type() {
        let state = PulseState::default_state();
        let rc = run_pactl_list(&["bogus".to_string()], &state);
        assert_eq!(rc, 1);
    }

    // -- PaplayOptions defaults ------------------------------------------------

    #[test]
    fn test_paplay_options_defaults() {
        let opts = PaplayOptions::new();
        assert_eq!(opts.channels, DEFAULT_CHANNELS);
        assert_eq!(opts.rate, DEFAULT_SAMPLE_RATE);
        assert_eq!(opts.format, SampleFormat::S16Le);
        assert!(opts.filename.is_none());
        assert!(opts.device.is_none());
        assert!(opts.volume.is_none());
        assert!(!opts.list_file_formats);
    }

    // -- ParecordOptions defaults ----------------------------------------------

    #[test]
    fn test_parecord_options_defaults() {
        let opts = ParecordOptions::new();
        assert_eq!(opts.channels, DEFAULT_CHANNELS);
        assert_eq!(opts.rate, DEFAULT_SAMPLE_RATE);
        assert_eq!(opts.format, SampleFormat::S16Le);
        assert!(opts.filename.is_none());
        assert!(opts.device.is_none());
        assert!(opts.volume.is_none());
        assert_eq!(opts._file_format, "wav");
    }

    // -- DaemonOptions defaults ------------------------------------------------

    #[test]
    fn test_daemon_options_defaults() {
        let opts = DaemonOptions::new();
        assert!(!opts._daemonize);
        assert!(!opts._system);
        assert!(opts._realtime);
        assert!(opts._high_priority);
        assert_eq!(opts._log_level, 1);
        assert_eq!(opts._exit_idle_time, 20);
        assert!(!opts.check);
        assert!(!opts.kill);
        assert!(!opts.start);
    }
}
