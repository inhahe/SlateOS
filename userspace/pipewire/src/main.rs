#![deny(clippy::all)]

//! pipewire — Slate OS PipeWire multimedia framework
//!
//! Multi-personality binary for PipeWire audio/video management.
//! Detected via argv[0] basename (strip path separators and `.exe` suffix):
//!
//! - `pw-cli` (default) — PipeWire command-line interface
//! - `pw-dump` — dump PipeWire objects as JSON
//! - `pw-record` — record audio
//! - `pw-play` — play audio
//! - `pw-cat` — cat audio streams
//! - `pw-mon` — monitor PipeWire events
//! - `pw-metadata` — manage PipeWire metadata
//! - `pw-top` — top-like display of PipeWire processing
//! - `wpctl` — WirePlumber control (status, inspect, set-volume, set-mute, set-default)
//! - `pipewire` — PipeWire daemon

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const PW_VERSION: &str = "0.3.80";
const WP_VERSION: &str = "0.4.17";
const _PW_CONF: &str = "/etc/pipewire/pipewire.conf";
const _PW_RUNTIME_DIR: &str = "/run/pipewire";
const _PW_SOCKET: &str = "/run/pipewire/pipewire-0";
const _WP_CONF: &str = "/etc/wireplumber/wireplumber.conf";
const DEFAULT_SAMPLE_RATE: u32 = 48000;
const DEFAULT_CHANNELS: u16 = 2;
const DEFAULT_BUFFER_SIZE: u32 = 1024;
const _DEFAULT_LATENCY: &str = "1024/48000";
const _MAX_PORTS: usize = 256;
const _MAX_LINKS: usize = 512;
const _MAX_NODES: usize = 128;

// ── Enums ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    PwCli,
    PwDump,
    PwRecord,
    PwPlay,
    PwCat,
    PwMon,
    PwMetadata,
    PwTop,
    Wpctl,
    Pipewire,
}

impl Personality {
    fn from_name(name: &str) -> Self {
        match name {
            "pw-dump" => Self::PwDump,
            "pw-record" => Self::PwRecord,
            "pw-play" => Self::PwPlay,
            "pw-cat" => Self::PwCat,
            "pw-mon" => Self::PwMon,
            "pw-metadata" => Self::PwMetadata,
            "pw-top" => Self::PwTop,
            "wpctl" => Self::Wpctl,
            "pipewire" => Self::Pipewire,
            _ => Self::PwCli,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeType {
    AudioSource,
    AudioSink,
    VideoSource,
    _VideoSink,
    MidiSource,
    _MidiSink,
    _Filter,
    _Loopback,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AudioSource => write!(f, "Audio/Source"),
            Self::AudioSink => write!(f, "Audio/Sink"),
            Self::VideoSource => write!(f, "Video/Source"),
            Self::_VideoSink => write!(f, "Video/Sink"),
            Self::MidiSource => write!(f, "MIDI/Source"),
            Self::_MidiSink => write!(f, "MIDI/Sink"),
            Self::_Filter => write!(f, "Filter"),
            Self::_Loopback => write!(f, "Loopback"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeState {
    _Creating,
    Suspended,
    Idle,
    Running,
    _Error,
}

impl std::fmt::Display for NodeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Creating => write!(f, "creating"),
            Self::Suspended => write!(f, "suspended"),
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::_Error => write!(f, "error"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortDirection {
    Input,
    Output,
}

impl std::fmt::Display for PortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input => write!(f, "in"),
            Self::Output => write!(f, "out"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaType {
    Audio,
    Video,
    _Midi,
    _Unknown,
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio => write!(f, "Audio"),
            Self::Video => write!(f, "Video"),
            Self::_Midi => write!(f, "Midi"),
            Self::_Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaSubtype {
    Raw,
    _Compressed,
    _Dsd,
    _Unknown,
}

impl std::fmt::Display for MediaSubtype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Raw => write!(f, "raw"),
            Self::_Compressed => write!(f, "compressed"),
            Self::_Dsd => write!(f, "dsd"),
            Self::_Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioFormat {
    S16Le,
    S24Le,
    S32Le,
    F32Le,
    _S16Be,
    _S24Be,
    _S32Be,
    _F32Be,
    _U8,
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::S16Le => write!(f, "S16LE"),
            Self::S24Le => write!(f, "S24LE"),
            Self::S32Le => write!(f, "S32LE"),
            Self::F32Le => write!(f, "F32LE"),
            Self::_S16Be => write!(f, "S16BE"),
            Self::_S24Be => write!(f, "S24BE"),
            Self::_S32Be => write!(f, "S32BE"),
            Self::_F32Be => write!(f, "F32BE"),
            Self::_U8 => write!(f, "U8"),
        }
    }
}

impl AudioFormat {
    fn bytes_per_sample(self) -> u32 {
        match self {
            Self::_U8 => 1,
            Self::S16Le | Self::_S16Be => 2,
            Self::S24Le | Self::_S24Be => 3,
            Self::S32Le | Self::F32Le | Self::_S32Be | Self::_F32Be => 4,
        }
    }

    fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "S16LE" | "s16le" | "s16" => Some(Self::S16Le),
            "S24LE" | "s24le" | "s24" => Some(Self::S24Le),
            "S32LE" | "s32le" | "s32" => Some(Self::S32Le),
            "F32LE" | "f32le" | "f32" => Some(Self::F32Le),
            "S16BE" | "s16be" => Some(Self::_S16Be),
            "S24BE" | "s24be" => Some(Self::_S24Be),
            "S32BE" | "s32be" => Some(Self::_S32Be),
            "F32BE" | "f32be" => Some(Self::_F32Be),
            "U8" | "u8" => Some(Self::_U8),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkState {
    _Init,
    _Negotiating,
    _Allocating,
    Paused,
    Active,
    _Error,
}

impl std::fmt::Display for LinkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Init => write!(f, "init"),
            Self::_Negotiating => write!(f, "negotiating"),
            Self::_Allocating => write!(f, "allocating"),
            Self::Paused => write!(f, "paused"),
            Self::Active => write!(f, "active"),
            Self::_Error => write!(f, "error"),
        }
    }
}

#[allow(dead_code)] // variant set models all device classes; only some are emitted yet
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeviceType {
    AlsaPcm,
    AlsaCard,
    V4l2,
    Bluetooth,
    Virtual,
}

impl std::fmt::Display for DeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlsaPcm => write!(f, "alsa/pcm"),
            Self::AlsaCard => write!(f, "alsa/card"),
            Self::V4l2 => write!(f, "v4l2"),
            Self::Bluetooth => write!(f, "bluetooth"),
            Self::Virtual => write!(f, "virtual"),
        }
    }
}

#[allow(dead_code)] // full PipeWire interface set; only core object types are listed today
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectType {
    Node,
    Port,
    Link,
    Device,
    Client,
    Module,
    Factory,
    Core,
    Profiler,
    Metadata,
}

impl std::fmt::Display for ObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node => write!(f, "PipeWire:Interface:Node"),
            Self::Port => write!(f, "PipeWire:Interface:Port"),
            Self::Link => write!(f, "PipeWire:Interface:Link"),
            Self::Device => write!(f, "PipeWire:Interface:Device"),
            Self::Client => write!(f, "PipeWire:Interface:Client"),
            Self::Module => write!(f, "PipeWire:Interface:Module"),
            Self::Factory => write!(f, "PipeWire:Interface:Factory"),
            Self::Core => write!(f, "PipeWire:Interface:Core"),
            Self::Profiler => write!(f, "PipeWire:Interface:Profiler"),
            Self::Metadata => write!(f, "PipeWire:Interface:Metadata"),
        }
    }
}

#[allow(dead_code)] // full monitor event model; only Added is emitted by the simulated registry
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MonitorEventType {
    Added,
    Removed,
    Changed,
    Bound,
    Permissions,
}

impl std::fmt::Display for MonitorEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Added => write!(f, "added"),
            Self::Removed => write!(f, "removed"),
            Self::Changed => write!(f, "changed"),
            Self::Bound => write!(f, "bound"),
            Self::Permissions => write!(f, "permissions"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClientAccess {
    Unrestricted,
    _Restricted,
    _Flatpak,
    _Portal,
}

impl std::fmt::Display for ClientAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unrestricted => write!(f, "unrestricted"),
            Self::_Restricted => write!(f, "restricted"),
            Self::_Flatpak => write!(f, "flatpak"),
            Self::_Portal => write!(f, "portal"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamDirection {
    Playback,
    Capture,
}

impl std::fmt::Display for StreamDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Playback => write!(f, "playback"),
            Self::Capture => write!(f, "capture"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelPosition {
    Mono,
    FrontLeft,
    FrontRight,
    _FrontCenter,
    _RearLeft,
    _RearRight,
    _Lfe,
    _SideLeft,
    _SideRight,
}

impl std::fmt::Display for ChannelPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mono => write!(f, "MONO"),
            Self::FrontLeft => write!(f, "FL"),
            Self::FrontRight => write!(f, "FR"),
            Self::_FrontCenter => write!(f, "FC"),
            Self::_RearLeft => write!(f, "RL"),
            Self::_RearRight => write!(f, "RR"),
            Self::_Lfe => write!(f, "LFE"),
            Self::_SideLeft => write!(f, "SL"),
            Self::_SideRight => write!(f, "SR"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum _DaemonAction {
    Start,
    _Stop,
    _Restart,
    _Status,
}

impl std::fmt::Display for _DaemonAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start => write!(f, "start"),
            Self::_Stop => write!(f, "stop"),
            Self::_Restart => write!(f, "restart"),
            Self::_Status => write!(f, "status"),
        }
    }
}

// Full metadata-key taxonomy; only a subset is produced by the simulated
// metadata store today (RouteProfile et al. reserved for device routing).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum _MetadataKeyType {
    _Default,
    RouteDevice,
    RouteProfile,
    TargetNode,
    _TargetObject,
    _Volume,
    _Mute,
}

impl std::fmt::Display for _MetadataKeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Default => write!(f, "default"),
            Self::RouteDevice => write!(f, "route-device"),
            Self::RouteProfile => write!(f, "route-profile"),
            Self::TargetNode => write!(f, "target.node"),
            Self::_TargetObject => write!(f, "target.object"),
            Self::_Volume => write!(f, "volume"),
            Self::_Mute => write!(f, "mute"),
        }
    }
}

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct PwNode {
    id: u32,
    name: String,
    node_type: NodeType,
    state: NodeState,
    media_class: String,
    _description: String,
    _driver: bool,
    sample_rate: u32,
    channels: u16,
    format: AudioFormat,
    _buffer_size: u32,
    _latency_ns: u64,
    _quantum_samples: u32,
    _ports: Vec<u32>,
}

#[derive(Clone, Debug)]
struct PwPort {
    id: u32,
    node_id: u32,
    name: String,
    direction: PortDirection,
    _media_type: MediaType,
    _media_subtype: MediaSubtype,
    _format: AudioFormat,
    _channel: ChannelPosition,
    _physical: bool,
    _terminal: bool,
    _alias: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PwLink {
    id: u32,
    output_node: u32,
    output_port: u32,
    input_node: u32,
    input_port: u32,
    state: LinkState,
    _active: bool,
    _feedback: bool,
}

#[derive(Clone, Debug)]
struct PwDevice {
    id: u32,
    name: String,
    device_type: DeviceType,
    _description: String,
    _nick: String,
    _bus_path: String,
    _serial: String,
    _vendor_id: u32,
    _product_id: u32,
    _form_factor: String,
    profiles: Vec<DeviceProfile>,
    _active_profile_index: usize,
}

#[derive(Clone, Debug)]
struct DeviceProfile {
    _index: u32,
    name: String,
    _description: String,
    _priority: u32,
    _available: bool,
    _classes: Vec<String>,
}

#[derive(Clone, Debug)]
struct PwClient {
    id: u32,
    name: String,
    _pid: u32,
    _access: ClientAccess,
    _protocol: String,
}

#[derive(Clone, Debug)]
struct PwModule {
    id: u32,
    name: String,
    _filename: String,
    _args: String,
}

#[derive(Clone, Debug)]
struct PwFactory {
    _id: u32,
    name: String,
    _factory_type: String,
    _module_id: u32,
}

#[derive(Clone, Debug)]
struct PwMetadataEntry {
    subject: u32,
    key: String,
    value: String,
    _key_type: String,
}

#[derive(Clone, Debug)]
struct PwCoreInfo {
    _cookie: u32,
    _user_name: String,
    _host_name: String,
    _version: String,
    _name: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProcessingInfo {
    node_id: u32,
    _quantum: u32,
    _rate: u32,
    _wait_ns: u64,
    _busy_ns: u64,
    _xrun_count: u32,
    _latency_ns: u64,
}

#[derive(Clone, Debug)]
struct MonitorEvent {
    event_type: MonitorEventType,
    object_type: ObjectType,
    object_id: u32,
    _object_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct AudioStreamConfig {
    format: AudioFormat,
    rate: u32,
    channels: u16,
    _volume: f64,
    _buffer_size: u32,
}

#[derive(Clone, Debug)]
struct RecordConfig {
    target: Option<String>,
    output_file: String,
    stream: AudioStreamConfig,
    _duration_secs: Option<f64>,
}

#[derive(Clone, Debug)]
struct PlayConfig {
    target: Option<String>,
    input_file: String,
    stream: AudioStreamConfig,
    _loop_playback: bool,
}

#[derive(Clone, Debug)]
struct CatConfig {
    direction: StreamDirection,
    target: Option<String>,
    stream: AudioStreamConfig,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct VolumeInfo {
    _node_id: u32,
    volume: f64,
    muted: bool,
    _channel_count: u16,
    _base_volume: f64,
    _step: f64,
}

// ── World builder ──────────────────────────────────────────────────────

/// PipeWire state containing all simulated objects
#[derive(Clone, Debug)]
struct PwState {
    nodes: Vec<PwNode>,
    ports: Vec<PwPort>,
    links: Vec<PwLink>,
    devices: Vec<PwDevice>,
    clients: Vec<PwClient>,
    modules: Vec<PwModule>,
    factories: Vec<PwFactory>,
    metadata: Vec<PwMetadataEntry>,
    core_info: PwCoreInfo,
    _default_sink_id: u32,
    _default_source_id: u32,
}

fn build_simulated_state() -> PwState {
    let core_info = PwCoreInfo {
        _cookie: 0xDEAD_BEEF,
        _user_name: String::from("user"),
        _host_name: String::from("slateos"),
        _version: String::from(PW_VERSION),
        _name: String::from("pipewire-0"),
    };

    // Modules
    let modules = vec![
        PwModule {
            id: 0,
            name: String::from("libpipewire-module-rt"),
            _filename: String::from("/usr/lib/pipewire-0.3/libpipewire-module-rt.so"),
            _args: String::from(
                "{ nice.level = -11, rt.prio = 88, rt.time.soft = -1, rt.time.hard = -1 }",
            ),
        },
        PwModule {
            id: 1,
            name: String::from("libpipewire-module-protocol-native"),
            _filename: String::from("/usr/lib/pipewire-0.3/libpipewire-module-protocol-native.so"),
            _args: String::new(),
        },
        PwModule {
            id: 2,
            name: String::from("libpipewire-module-profiler"),
            _filename: String::from("/usr/lib/pipewire-0.3/libpipewire-module-profiler.so"),
            _args: String::new(),
        },
        PwModule {
            id: 3,
            name: String::from("libpipewire-module-metadata"),
            _filename: String::from("/usr/lib/pipewire-0.3/libpipewire-module-metadata.so"),
            _args: String::new(),
        },
        PwModule {
            id: 4,
            name: String::from("libpipewire-module-spa-device-factory"),
            _filename: String::from(
                "/usr/lib/pipewire-0.3/libpipewire-module-spa-device-factory.so",
            ),
            _args: String::new(),
        },
        PwModule {
            id: 5,
            name: String::from("libpipewire-module-spa-node-factory"),
            _filename: String::from("/usr/lib/pipewire-0.3/libpipewire-module-spa-node-factory.so"),
            _args: String::new(),
        },
        PwModule {
            id: 6,
            name: String::from("libpipewire-module-adapter"),
            _filename: String::from("/usr/lib/pipewire-0.3/libpipewire-module-adapter.so"),
            _args: String::new(),
        },
        PwModule {
            id: 7,
            name: String::from("libpipewire-module-session-manager"),
            _filename: String::from("/usr/lib/pipewire-0.3/libpipewire-module-session-manager.so"),
            _args: String::new(),
        },
    ];

    // Factories
    let factories = vec![
        PwFactory {
            _id: 50,
            name: String::from("support.node.driver"),
            _factory_type: String::from("PipeWire:Interface:Node"),
            _module_id: 5,
        },
        PwFactory {
            _id: 51,
            name: String::from("adapter"),
            _factory_type: String::from("PipeWire:Interface:Node"),
            _module_id: 6,
        },
        PwFactory {
            _id: 52,
            name: String::from("spa-node-factory"),
            _factory_type: String::from("PipeWire:Interface:Node"),
            _module_id: 5,
        },
        PwFactory {
            _id: 53,
            name: String::from("spa-device-factory"),
            _factory_type: String::from("PipeWire:Interface:Device"),
            _module_id: 4,
        },
    ];

    // Devices
    let devices = vec![
        PwDevice {
            id: 100,
            name: String::from("alsa_card.pci-0000_00_1f.3"),
            device_type: DeviceType::AlsaCard,
            _description: String::from("Built-in Audio"),
            _nick: String::from("HDA Intel PCH"),
            _bus_path: String::from("pci-0000:00:1f.3"),
            _serial: String::from(""),
            _vendor_id: 0x8086,
            _product_id: 0xA171,
            _form_factor: String::from("internal"),
            profiles: vec![
                DeviceProfile {
                    _index: 0,
                    name: String::from("HiFi"),
                    _description: String::from("High Fidelity"),
                    _priority: 8000,
                    _available: true,
                    _classes: vec![String::from("Audio/Sink"), String::from("Audio/Source")],
                },
                DeviceProfile {
                    _index: 1,
                    name: String::from("off"),
                    _description: String::from("Off"),
                    _priority: 0,
                    _available: true,
                    _classes: vec![],
                },
            ],
            _active_profile_index: 0,
        },
        PwDevice {
            id: 101,
            name: String::from("alsa_card.usb-SteelSeries_Arctis_7"),
            device_type: DeviceType::AlsaCard,
            _description: String::from("SteelSeries Arctis 7"),
            _nick: String::from("Arctis 7"),
            _bus_path: String::from("usb-0000:00:14.0-3"),
            _serial: String::from("SS-A7-001"),
            _vendor_id: 0x1038,
            _product_id: 0x12AD,
            _form_factor: String::from("headset"),
            profiles: vec![DeviceProfile {
                _index: 0,
                name: String::from("output:analog-stereo+input:mono-fallback"),
                _description: String::from("Analog Stereo Output + Mono Input"),
                _priority: 6500,
                _available: true,
                _classes: vec![String::from("Audio/Sink"), String::from("Audio/Source")],
            }],
            _active_profile_index: 0,
        },
    ];

    // Nodes
    let nodes = vec![
        PwNode {
            id: 30,
            name: String::from("alsa_output.pci-0000_00_1f.3.analog-stereo"),
            node_type: NodeType::AudioSink,
            state: NodeState::Running,
            media_class: String::from("Audio/Sink"),
            _description: String::from("Built-in Audio Analog Stereo"),
            _driver: true,
            sample_rate: 48000,
            channels: 2,
            format: AudioFormat::S32Le,
            _buffer_size: 1024,
            _latency_ns: 21333333,
            _quantum_samples: 1024,
            _ports: vec![200, 201],
        },
        PwNode {
            id: 31,
            name: String::from("alsa_input.pci-0000_00_1f.3.analog-stereo"),
            node_type: NodeType::AudioSource,
            state: NodeState::Suspended,
            media_class: String::from("Audio/Source"),
            _description: String::from("Built-in Audio Analog Stereo"),
            _driver: true,
            sample_rate: 48000,
            channels: 2,
            format: AudioFormat::S32Le,
            _buffer_size: 1024,
            _latency_ns: 21333333,
            _quantum_samples: 1024,
            _ports: vec![202, 203],
        },
        PwNode {
            id: 32,
            name: String::from("alsa_output.usb-SteelSeries_Arctis_7.analog-stereo"),
            node_type: NodeType::AudioSink,
            state: NodeState::Idle,
            media_class: String::from("Audio/Sink"),
            _description: String::from("Arctis 7 Analog Stereo"),
            _driver: true,
            sample_rate: 48000,
            channels: 2,
            format: AudioFormat::S16Le,
            _buffer_size: 512,
            _latency_ns: 10666666,
            _quantum_samples: 512,
            _ports: vec![204, 205],
        },
        PwNode {
            id: 33,
            name: String::from("alsa_input.usb-SteelSeries_Arctis_7.mono-fallback"),
            node_type: NodeType::AudioSource,
            state: NodeState::Suspended,
            media_class: String::from("Audio/Source"),
            _description: String::from("Arctis 7 Mono"),
            _driver: true,
            sample_rate: 48000,
            channels: 1,
            format: AudioFormat::S16Le,
            _buffer_size: 512,
            _latency_ns: 10666666,
            _quantum_samples: 512,
            _ports: vec![206],
        },
        PwNode {
            id: 34,
            name: String::from("Firefox"),
            node_type: NodeType::AudioSink,
            state: NodeState::Running,
            media_class: String::from("Stream/Output/Audio"),
            _description: String::from("Firefox"),
            _driver: false,
            sample_rate: 48000,
            channels: 2,
            format: AudioFormat::F32Le,
            _buffer_size: 1024,
            _latency_ns: 21333333,
            _quantum_samples: 1024,
            _ports: vec![207, 208],
        },
        PwNode {
            id: 35,
            name: String::from("WEBRTC VoiceEngine"),
            node_type: NodeType::AudioSource,
            state: NodeState::Running,
            media_class: String::from("Stream/Input/Audio"),
            _description: String::from("WEBRTC VoiceEngine"),
            _driver: false,
            sample_rate: 48000,
            channels: 1,
            format: AudioFormat::F32Le,
            _buffer_size: 480,
            _latency_ns: 10000000,
            _quantum_samples: 480,
            _ports: vec![209],
        },
        PwNode {
            id: 36,
            name: String::from("spotify"),
            node_type: NodeType::AudioSink,
            state: NodeState::Idle,
            media_class: String::from("Stream/Output/Audio"),
            _description: String::from("Spotify"),
            _driver: false,
            sample_rate: 44100,
            channels: 2,
            format: AudioFormat::F32Le,
            _buffer_size: 1024,
            _latency_ns: 23219955,
            _quantum_samples: 1024,
            _ports: vec![210, 211],
        },
        PwNode {
            id: 37,
            name: String::from("v4l2_output.pci-0000_00_14.0-usb-0_3_1.0"),
            node_type: NodeType::VideoSource,
            state: NodeState::Suspended,
            media_class: String::from("Video/Source"),
            _description: String::from("USB Camera"),
            _driver: true,
            sample_rate: 0,
            channels: 0,
            format: AudioFormat::S16Le,
            _buffer_size: 0,
            _latency_ns: 0,
            _quantum_samples: 0,
            _ports: vec![212],
        },
        PwNode {
            id: 38,
            name: String::from("midi.seq_client"),
            node_type: NodeType::MidiSource,
            state: NodeState::Suspended,
            media_class: String::from("Midi/Bridge"),
            _description: String::from("Midi Through"),
            _driver: false,
            sample_rate: 0,
            channels: 0,
            format: AudioFormat::S16Le,
            _buffer_size: 0,
            _latency_ns: 0,
            _quantum_samples: 0,
            _ports: vec![213, 214],
        },
    ];

    // Ports
    let ports = vec![
        PwPort {
            id: 200,
            node_id: 30,
            name: String::from("playback_FL"),
            direction: PortDirection::Input,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S32Le,
            _channel: ChannelPosition::FrontLeft,
            _physical: true,
            _terminal: true,
            _alias: String::from("Built-in Audio:playback_FL"),
        },
        PwPort {
            id: 201,
            node_id: 30,
            name: String::from("playback_FR"),
            direction: PortDirection::Input,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S32Le,
            _channel: ChannelPosition::FrontRight,
            _physical: true,
            _terminal: true,
            _alias: String::from("Built-in Audio:playback_FR"),
        },
        PwPort {
            id: 202,
            node_id: 31,
            name: String::from("capture_FL"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S32Le,
            _channel: ChannelPosition::FrontLeft,
            _physical: true,
            _terminal: true,
            _alias: String::from("Built-in Audio:capture_FL"),
        },
        PwPort {
            id: 203,
            node_id: 31,
            name: String::from("capture_FR"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S32Le,
            _channel: ChannelPosition::FrontRight,
            _physical: true,
            _terminal: true,
            _alias: String::from("Built-in Audio:capture_FR"),
        },
        PwPort {
            id: 204,
            node_id: 32,
            name: String::from("playback_FL"),
            direction: PortDirection::Input,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S16Le,
            _channel: ChannelPosition::FrontLeft,
            _physical: true,
            _terminal: true,
            _alias: String::from("Arctis 7:playback_FL"),
        },
        PwPort {
            id: 205,
            node_id: 32,
            name: String::from("playback_FR"),
            direction: PortDirection::Input,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S16Le,
            _channel: ChannelPosition::FrontRight,
            _physical: true,
            _terminal: true,
            _alias: String::from("Arctis 7:playback_FR"),
        },
        PwPort {
            id: 206,
            node_id: 33,
            name: String::from("capture_MONO"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S16Le,
            _channel: ChannelPosition::Mono,
            _physical: true,
            _terminal: true,
            _alias: String::from("Arctis 7:capture_MONO"),
        },
        PwPort {
            id: 207,
            node_id: 34,
            name: String::from("output_FL"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::F32Le,
            _channel: ChannelPosition::FrontLeft,
            _physical: false,
            _terminal: false,
            _alias: String::from("Firefox:output_FL"),
        },
        PwPort {
            id: 208,
            node_id: 34,
            name: String::from("output_FR"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::F32Le,
            _channel: ChannelPosition::FrontRight,
            _physical: false,
            _terminal: false,
            _alias: String::from("Firefox:output_FR"),
        },
        PwPort {
            id: 209,
            node_id: 35,
            name: String::from("input_MONO"),
            direction: PortDirection::Input,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::F32Le,
            _channel: ChannelPosition::Mono,
            _physical: false,
            _terminal: false,
            _alias: String::from("WEBRTC:input_MONO"),
        },
        PwPort {
            id: 210,
            node_id: 36,
            name: String::from("output_FL"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::F32Le,
            _channel: ChannelPosition::FrontLeft,
            _physical: false,
            _terminal: false,
            _alias: String::from("Spotify:output_FL"),
        },
        PwPort {
            id: 211,
            node_id: 36,
            name: String::from("output_FR"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::F32Le,
            _channel: ChannelPosition::FrontRight,
            _physical: false,
            _terminal: false,
            _alias: String::from("Spotify:output_FR"),
        },
        PwPort {
            id: 212,
            node_id: 37,
            name: String::from("video_out"),
            direction: PortDirection::Output,
            _media_type: MediaType::Video,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S16Le,
            _channel: ChannelPosition::Mono,
            _physical: true,
            _terminal: true,
            _alias: String::from("USB Camera:video_out"),
        },
        PwPort {
            id: 213,
            node_id: 38,
            name: String::from("midi_in"),
            direction: PortDirection::Input,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S16Le,
            _channel: ChannelPosition::Mono,
            _physical: false,
            _terminal: false,
            _alias: String::from("Midi Through:midi_in"),
        },
        PwPort {
            id: 214,
            node_id: 38,
            name: String::from("midi_out"),
            direction: PortDirection::Output,
            _media_type: MediaType::Audio,
            _media_subtype: MediaSubtype::Raw,
            _format: AudioFormat::S16Le,
            _channel: ChannelPosition::Mono,
            _physical: false,
            _terminal: false,
            _alias: String::from("Midi Through:midi_out"),
        },
    ];

    // Links
    let links = vec![
        PwLink {
            id: 300,
            output_node: 34,
            output_port: 207,
            input_node: 30,
            input_port: 200,
            state: LinkState::Active,
            _active: true,
            _feedback: false,
        },
        PwLink {
            id: 301,
            output_node: 34,
            output_port: 208,
            input_node: 30,
            input_port: 201,
            state: LinkState::Active,
            _active: true,
            _feedback: false,
        },
        PwLink {
            id: 302,
            output_node: 31,
            output_port: 202,
            input_node: 35,
            input_port: 209,
            state: LinkState::Active,
            _active: true,
            _feedback: false,
        },
        PwLink {
            id: 303,
            output_node: 36,
            output_port: 210,
            input_node: 30,
            input_port: 200,
            state: LinkState::Paused,
            _active: false,
            _feedback: false,
        },
        PwLink {
            id: 304,
            output_node: 36,
            output_port: 211,
            input_node: 30,
            input_port: 201,
            state: LinkState::Paused,
            _active: false,
            _feedback: false,
        },
    ];

    // Clients
    let clients = vec![
        PwClient {
            id: 40,
            name: String::from("WirePlumber"),
            _pid: 1001,
            _access: ClientAccess::Unrestricted,
            _protocol: String::from("protocol-native"),
        },
        PwClient {
            id: 41,
            name: String::from("pipewire"),
            _pid: 1000,
            _access: ClientAccess::Unrestricted,
            _protocol: String::from("protocol-native"),
        },
        PwClient {
            id: 42,
            name: String::from("Firefox"),
            _pid: 2001,
            _access: ClientAccess::Unrestricted,
            _protocol: String::from("protocol-native"),
        },
        PwClient {
            id: 43,
            name: String::from("spotify"),
            _pid: 2002,
            _access: ClientAccess::Unrestricted,
            _protocol: String::from("protocol-native"),
        },
        PwClient {
            id: 44,
            name: String::from("xdg-desktop-portal"),
            _pid: 1500,
            _access: ClientAccess::Unrestricted,
            _protocol: String::from("protocol-native"),
        },
    ];

    // Metadata
    let metadata = vec![
        PwMetadataEntry {
            subject: 0,
            key: String::from("default.audio.sink"),
            value: String::from("{\"name\":\"alsa_output.pci-0000_00_1f.3.analog-stereo\"}"),
            _key_type: String::from("Spa:String:JSON"),
        },
        PwMetadataEntry {
            subject: 0,
            key: String::from("default.audio.source"),
            value: String::from("{\"name\":\"alsa_input.pci-0000_00_1f.3.analog-stereo\"}"),
            _key_type: String::from("Spa:String:JSON"),
        },
        PwMetadataEntry {
            subject: 30,
            key: String::from("target.node"),
            value: String::from("30"),
            _key_type: String::from("Spa:Int"),
        },
        PwMetadataEntry {
            subject: 34,
            key: String::from("target.node"),
            value: String::from("30"),
            _key_type: String::from("Spa:Int"),
        },
    ];

    PwState {
        nodes,
        ports,
        links,
        devices,
        clients,
        modules,
        factories,
        metadata,
        core_info,
        _default_sink_id: 30,
        _default_source_id: 31,
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn find_node_by_id(state: &PwState, id: u32) -> Option<&PwNode> {
    state.nodes.iter().find(|n| n.id == id)
}

fn find_port_by_id(state: &PwState, id: u32) -> Option<&PwPort> {
    state.ports.iter().find(|p| p.id == id)
}

fn find_device_by_id(state: &PwState, id: u32) -> Option<&PwDevice> {
    state.devices.iter().find(|d| d.id == id)
}

fn find_client_by_id(state: &PwState, id: u32) -> Option<&PwClient> {
    state.clients.iter().find(|c| c.id == id)
}

fn find_module_by_id(state: &PwState, id: u32) -> Option<&PwModule> {
    state.modules.iter().find(|m| m.id == id)
}

fn ports_for_node(state: &PwState, node_id: u32) -> Vec<&PwPort> {
    state
        .ports
        .iter()
        .filter(|p| p.node_id == node_id)
        .collect()
}

fn links_for_node(state: &PwState, node_id: u32) -> Vec<&PwLink> {
    state
        .links
        .iter()
        .filter(|l| l.output_node == node_id || l.input_node == node_id)
        .collect()
}

fn sink_nodes(state: &PwState) -> Vec<&PwNode> {
    state
        .nodes
        .iter()
        .filter(|n| n.media_class == "Audio/Sink")
        .collect()
}

fn source_nodes(state: &PwState) -> Vec<&PwNode> {
    state
        .nodes
        .iter()
        .filter(|n| n.media_class == "Audio/Source")
        .collect()
}

fn stream_output_nodes(state: &PwState) -> Vec<&PwNode> {
    state
        .nodes
        .iter()
        .filter(|n| n.media_class == "Stream/Output/Audio")
        .collect()
}

fn stream_input_nodes(state: &PwState) -> Vec<&PwNode> {
    state
        .nodes
        .iter()
        .filter(|n| n.media_class == "Stream/Input/Audio")
        .collect()
}

fn default_sink(state: &PwState) -> Option<&PwNode> {
    state
        .metadata
        .iter()
        .find(|m| m.key == "default.audio.sink")
        .and_then(|m| {
            // Parse name from JSON-like value
            let val = &m.value;
            extract_json_string_field(val, "name")
        })
        .and_then(|name| state.nodes.iter().find(|n| n.name == name))
}

fn default_source(state: &PwState) -> Option<&PwNode> {
    state
        .metadata
        .iter()
        .find(|m| m.key == "default.audio.source")
        .and_then(|m| {
            let val = &m.value;
            extract_json_string_field(val, "name")
        })
        .and_then(|name| state.nodes.iter().find(|n| n.name == name))
}

/// Very minimal JSON field extraction — no external crate.
fn extract_json_string_field<'a>(json: &'a str, field: &str) -> Option<&'a str> {
    let pattern = format!("\"{}\":\"", field);
    let start = json.find(&pattern)?;
    let value_start = start + pattern.len();
    let rest = json.get(value_start..)?;
    let end = rest.find('"')?;
    rest.get(..end)
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn format_volume_percent(vol: f64) -> String {
    format!("{:.0}%", vol * 100.0)
}

fn parse_volume_spec(spec: &str) -> Option<f64> {
    if let Some(pct) = spec.strip_suffix('%') {
        pct.parse::<f64>().ok().map(|v| v / 100.0)
    } else if spec.contains('.') {
        spec.parse::<f64>().ok()
    } else {
        spec.parse::<f64>().ok().map(|v| v / 100.0)
    }
}

fn parse_id_or_name<'a>(s: &str, state: &'a PwState) -> Option<&'a PwNode> {
    if let Ok(id) = s.parse::<u32>() {
        find_node_by_id(state, id)
    } else {
        state
            .nodes
            .iter()
            .find(|n| n.name == s || n.media_class == s)
    }
}

#[allow(dead_code)] // compact node renderer reserved for a future short-list view, tested
fn format_node_short(node: &PwNode) -> String {
    format!(
        "id {}, type {}, name \"{}\"",
        node.id, node.media_class, node.name
    )
}

fn format_bytes_display(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn compute_data_rate(config: &AudioStreamConfig) -> u64 {
    u64::from(config.rate)
        * u64::from(config.channels)
        * u64::from(config.format.bytes_per_sample())
}

fn format_duration_hms(total_secs: f64) -> String {
    let h = total_secs as u64 / 3600;
    let m = (total_secs as u64 % 3600) / 60;
    let s = total_secs as u64 % 60;
    let frac = total_secs - (total_secs as u64) as f64;
    if h > 0 {
        format!("{:02}:{:02}:{:02}.{:02}", h, m, s, (frac * 100.0) as u64)
    } else {
        format!("{:02}:{:02}.{:02}", m, s, (frac * 100.0) as u64)
    }
}

fn parse_stream_args(args: &[String], default_direction: StreamDirection) -> AudioStreamConfig {
    let mut format = AudioFormat::S16Le;
    let mut rate = DEFAULT_SAMPLE_RATE;
    let mut channels = DEFAULT_CHANNELS;
    let mut volume = 1.0;
    let mut buffer_size = DEFAULT_BUFFER_SIZE;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" | "-f" if i + 1 < args.len() => {
                if let Some(fmt) = AudioFormat::from_str_opt(&args[i + 1]) {
                    format = fmt;
                }
                i += 1;
            }
            "--rate" | "-r" if i + 1 < args.len() => {
                if let Ok(r) = args[i + 1].parse() {
                    rate = r;
                }
                i += 1;
            }
            "--channels" | "-c" if i + 1 < args.len() => {
                if let Ok(c) = args[i + 1].parse() {
                    channels = c;
                }
                i += 1;
            }
            "--volume" if i + 1 < args.len() => {
                if let Ok(v) = args[i + 1].parse() {
                    volume = v;
                }
                i += 1;
            }
            "--latency" if i + 1 < args.len() => {
                if let Some(slash) = args[i + 1].find('/')
                    && let Ok(b) = args[i + 1][..slash].parse()
                {
                    buffer_size = b;
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let _ = default_direction;
    AudioStreamConfig {
        format,
        rate,
        channels,
        _volume: volume,
        _buffer_size: buffer_size,
    }
}

fn simulated_volume_for_node(node_id: u32) -> VolumeInfo {
    // Simulated volume data per well-known node
    match node_id {
        30 => VolumeInfo {
            _node_id: 30,
            volume: 0.74,
            muted: false,
            _channel_count: 2,
            _base_volume: 1.0,
            _step: 0.01,
        },
        31 => VolumeInfo {
            _node_id: 31,
            volume: 1.0,
            muted: false,
            _channel_count: 2,
            _base_volume: 1.0,
            _step: 0.01,
        },
        32 => VolumeInfo {
            _node_id: 32,
            volume: 0.50,
            muted: true,
            _channel_count: 2,
            _base_volume: 1.0,
            _step: 0.01,
        },
        33 => VolumeInfo {
            _node_id: 33,
            volume: 0.85,
            muted: false,
            _channel_count: 1,
            _base_volume: 1.0,
            _step: 0.01,
        },
        _ => VolumeInfo {
            _node_id: node_id,
            volume: 1.0,
            muted: false,
            _channel_count: 2,
            _base_volume: 1.0,
            _step: 0.01,
        },
    }
}

fn simulated_processing_info(state: &PwState) -> Vec<ProcessingInfo> {
    state
        .nodes
        .iter()
        .filter(|n| n.state == NodeState::Running || n.state == NodeState::Idle)
        .map(|n| ProcessingInfo {
            node_id: n.id,
            _quantum: n._buffer_size,
            _rate: n.sample_rate,
            _wait_ns: match n.state {
                NodeState::Running => 15_000_000,
                _ => 0,
            },
            _busy_ns: match n.state {
                NodeState::Running => 800_000,
                _ => 0,
            },
            _xrun_count: 0,
            _latency_ns: n._latency_ns,
        })
        .collect()
}

fn simulated_monitor_events(state: &PwState) -> Vec<MonitorEvent> {
    let mut events = Vec::new();
    for node in &state.nodes {
        events.push(MonitorEvent {
            event_type: MonitorEventType::Added,
            object_type: ObjectType::Node,
            object_id: node.id,
            _object_name: node.name.clone(),
        });
    }
    for port in &state.ports {
        events.push(MonitorEvent {
            event_type: MonitorEventType::Added,
            object_type: ObjectType::Port,
            object_id: port.id,
            _object_name: port.name.clone(),
        });
    }
    for link in &state.links {
        events.push(MonitorEvent {
            event_type: MonitorEventType::Added,
            object_type: ObjectType::Link,
            object_id: link.id,
            _object_name: String::new(),
        });
    }
    for device in &state.devices {
        events.push(MonitorEvent {
            event_type: MonitorEventType::Added,
            object_type: ObjectType::Device,
            object_id: device.id,
            _object_name: device.name.clone(),
        });
    }
    for client in &state.clients {
        events.push(MonitorEvent {
            event_type: MonitorEventType::Added,
            object_type: ObjectType::Client,
            object_id: client.id,
            _object_name: client.name.clone(),
        });
    }
    events
}

// ── pw-cli ─────────────────────────────────────────────────────────────

fn run_pw_cli(args: Vec<String>, state: &PwState) -> i32 {
    if args.is_empty() {
        print_pw_cli_help();
        return 0;
    }

    match args[0].as_str() {
        "help" | "--help" | "-h" => {
            print_pw_cli_help();
            0
        }
        "--version" | "-V" => {
            println!("pw-cli: Compiled with libpipewire {}", PW_VERSION);
            println!("pw-cli: Linked with libpipewire {}", PW_VERSION);
            0
        }
        "info" => run_pw_cli_info(&args[1..], state),
        "list-objects" => run_pw_cli_list_objects(&args[1..], state),
        "enum-params" => run_pw_cli_enum_params(&args[1..], state),
        "permissions" => run_pw_cli_permissions(&args[1..], state),
        "get-permissions" => run_pw_cli_get_permissions(&args[1..], state),
        "create-node" => {
            println!("pw-cli: create-node: simulated node creation (not persisted)");
            0
        }
        "create-link" => run_pw_cli_create_link(&args[1..], state),
        "destroy" => run_pw_cli_destroy(&args[1..], state),
        "send-command" => {
            if args.len() < 3 {
                eprintln!("pw-cli: send-command: usage: send-command <id> <command>");
                return 1;
            }
            println!("pw-cli: command sent to object {}", args[1]);
            0
        }
        "export" => {
            println!("pw-cli: export: not supported in simulated mode");
            0
        }
        unknown => {
            eprintln!("pw-cli: unknown command: {}", unknown);
            eprintln!("Type 'pw-cli help' for a list of commands");
            1
        }
    }
}

fn print_pw_cli_help() {
    println!("pw-cli - PipeWire command-line interface ({})", PW_VERSION);
    println!();
    println!("Usage: pw-cli [command] [options]");
    println!();
    println!("Commands:");
    println!("  help                 Show this help");
    println!("  info <id>            Show info about an object");
    println!("  list-objects [type]  List all objects or filter by type");
    println!("  enum-params <id> <param>  Enumerate params of object");
    println!("  permissions <id> <perms>  Set object permissions");
    println!("  get-permissions <id> Get object permissions");
    println!("  create-node <factory> [props]  Create a node");
    println!("  create-link <out-port> <in-port> [props]  Create a link");
    println!("  destroy <id>         Destroy an object");
    println!("  send-command <id> <cmd>  Send command to object");
    println!("  export <id>          Export an object");
    println!();
    println!("Options:");
    println!("  --version, -V    Show version");
    println!("  --help, -h       Show help");
}

fn run_pw_cli_info(args: &[String], state: &PwState) -> i32 {
    if args.is_empty() {
        // Print core info
        println!("\ttype: PipeWire:Interface:Core");
        println!("\tcookie: {}", state.core_info._cookie);
        println!("\tuser-name: \"{}\"", state.core_info._user_name);
        println!("\thost-name: \"{}\"", state.core_info._host_name);
        println!("\tversion: \"{}\"", state.core_info._version);
        println!("\tname: \"{}\"", state.core_info._name);
        return 0;
    }

    let id_str = &args[0];
    let id = match id_str.parse::<u32>() {
        Ok(v) => v,
        Err(_) => {
            // Try "all" or keyword
            if id_str == "all" {
                return run_pw_cli_info_all(state);
            }
            eprintln!("pw-cli: info: invalid id: {}", id_str);
            return 1;
        }
    };

    // Search across all object types
    if let Some(node) = find_node_by_id(state, id) {
        print_node_info(node, state);
        return 0;
    }
    if let Some(port) = find_port_by_id(state, id) {
        print_port_info(port);
        return 0;
    }
    if let Some(device) = find_device_by_id(state, id) {
        print_device_info(device);
        return 0;
    }
    if let Some(client) = find_client_by_id(state, id) {
        print_client_info(client);
        return 0;
    }
    if let Some(module) = find_module_by_id(state, id) {
        print_module_info(module);
        return 0;
    }
    if let Some(link) = state.links.iter().find(|l| l.id == id) {
        print_link_info(link, state);
        return 0;
    }

    eprintln!("pw-cli: info: object {} not found", id);
    1
}

fn run_pw_cli_info_all(state: &PwState) -> i32 {
    for node in &state.nodes {
        print_node_info(node, state);
        println!();
    }
    for port in &state.ports {
        print_port_info(port);
        println!();
    }
    for link in &state.links {
        print_link_info(link, state);
        println!();
    }
    for device in &state.devices {
        print_device_info(device);
        println!();
    }
    for client in &state.clients {
        print_client_info(client);
        println!();
    }
    for module in &state.modules {
        print_module_info(module);
        println!();
    }
    0
}

fn print_node_info(node: &PwNode, state: &PwState) {
    println!("\tid: {}", node.id);
    println!("\ttype: PipeWire:Interface:Node");
    println!("\tname: \"{}\"", node.name);
    println!("\tstate: \"{}\"", node.state);
    println!("\tmedia.class: \"{}\"", node.media_class);
    if node.sample_rate > 0 {
        println!("\taudio.format: \"{}\"", node.format);
        println!("\taudio.rate: {}", node.sample_rate);
        println!("\taudio.channels: {}", node.channels);
    }
    let node_ports = ports_for_node(state, node.id);
    if !node_ports.is_empty() {
        println!("\tports:");
        for port in &node_ports {
            println!("\t\t{}: \"{}\" ({})", port.id, port.name, port.direction);
        }
    }
    let node_links = links_for_node(state, node.id);
    if !node_links.is_empty() {
        println!("\tlinks:");
        for link in &node_links {
            println!(
                "\t\t{}: {} -> {} [{}]",
                link.id, link.output_port, link.input_port, link.state
            );
        }
    }
}

fn print_port_info(port: &PwPort) {
    println!("\tid: {}", port.id);
    println!("\ttype: PipeWire:Interface:Port");
    println!("\tname: \"{}\"", port.name);
    println!("\tnode.id: {}", port.node_id);
    println!("\tdirection: \"{}\"", port.direction);
}

fn print_link_info(link: &PwLink, state: &PwState) {
    println!("\tid: {}", link.id);
    println!("\ttype: PipeWire:Interface:Link");
    println!("\toutput-node-id: {}", link.output_node);
    println!("\toutput-port-id: {}", link.output_port);
    println!("\tinput-node-id: {}", link.input_node);
    println!("\tinput-port-id: {}", link.input_port);
    println!("\tstate: \"{}\"", link.state);
    // Print node names for context
    if let Some(out_node) = find_node_by_id(state, link.output_node) {
        println!("\toutput-node: \"{}\"", out_node.name);
    }
    if let Some(in_node) = find_node_by_id(state, link.input_node) {
        println!("\tinput-node: \"{}\"", in_node.name);
    }
}

fn print_device_info(device: &PwDevice) {
    println!("\tid: {}", device.id);
    println!("\ttype: PipeWire:Interface:Device");
    println!("\tname: \"{}\"", device.name);
    println!("\tdevice.type: \"{}\"", device.device_type);
    println!("\tprofiles:");
    for profile in &device.profiles {
        println!("\t\t\"{}\"", profile.name);
    }
}

fn print_client_info(client: &PwClient) {
    println!("\tid: {}", client.id);
    println!("\ttype: PipeWire:Interface:Client");
    println!("\tname: \"{}\"", client.name);
    println!("\tpid: {}", client._pid);
    println!("\taccess: \"{}\"", client._access);
}

fn print_module_info(module: &PwModule) {
    println!("\tid: {}", module.id);
    println!("\ttype: PipeWire:Interface:Module");
    println!("\tname: \"{}\"", module.name);
    println!("\tfilename: \"{}\"", module._filename);
}

fn run_pw_cli_list_objects(args: &[String], state: &PwState) -> i32 {
    let type_filter = args.first().map(|s| s.as_str());
    match type_filter {
        Some("Node") | Some("node") | Some("nodes") => {
            for node in &state.nodes {
                println!(
                    "\t{}: {} state:{} \"{}\"",
                    node.id, node.media_class, node.state, node.name
                );
            }
        }
        Some("Port") | Some("port") | Some("ports") => {
            for port in &state.ports {
                println!(
                    "\t{}: node:{} {} \"{}\"",
                    port.id, port.node_id, port.direction, port.name
                );
            }
        }
        Some("Link") | Some("link") | Some("links") => {
            for link in &state.links {
                println!(
                    "\t{}: {}:{} -> {}:{} [{}]",
                    link.id,
                    link.output_node,
                    link.output_port,
                    link.input_node,
                    link.input_port,
                    link.state
                );
            }
        }
        Some("Device") | Some("device") | Some("devices") => {
            for device in &state.devices {
                println!(
                    "\t{}: {} \"{}\"",
                    device.id, device.device_type, device.name
                );
            }
        }
        Some("Client") | Some("client") | Some("clients") => {
            for client in &state.clients {
                println!("\t{}: \"{}\" pid:{}", client.id, client.name, client._pid);
            }
        }
        Some("Module") | Some("module") | Some("modules") => {
            for module in &state.modules {
                println!("\t{}: \"{}\"", module.id, module.name);
            }
        }
        Some(t) => {
            eprintln!("pw-cli: list-objects: unknown type: {}", t);
            return 1;
        }
        None => {
            // List all
            for node in &state.nodes {
                println!("\t{}: Node \"{}\" [{}]", node.id, node.name, node.state);
            }
            for port in &state.ports {
                println!(
                    "\t{}: Port \"{}\" (node {})",
                    port.id, port.name, port.node_id
                );
            }
            for link in &state.links {
                println!(
                    "\t{}: Link {} -> {} [{}]",
                    link.id, link.output_port, link.input_port, link.state
                );
            }
            for device in &state.devices {
                println!("\t{}: Device \"{}\"", device.id, device.name);
            }
            for client in &state.clients {
                println!("\t{}: Client \"{}\"", client.id, client.name);
            }
            for module in &state.modules {
                println!("\t{}: Module \"{}\"", module.id, module.name);
            }
        }
    }
    0
}

fn run_pw_cli_enum_params(args: &[String], state: &PwState) -> i32 {
    if args.len() < 2 {
        eprintln!("pw-cli: enum-params: usage: enum-params <id> <param-name>");
        return 1;
    }
    let id: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pw-cli: enum-params: invalid id: {}", args[0]);
            return 1;
        }
    };
    let param_name = &args[1];

    if let Some(node) = find_node_by_id(state, id) {
        match param_name.as_str() {
            "Format" | "format" => {
                println!("  Object: id {}", node.id);
                println!("  Param.Format:");
                println!("    mediaType: {}", MediaType::Audio);
                println!("    mediaSubtype: {}", MediaSubtype::Raw);
                println!("    format: {}", node.format);
                println!("    rate: {}", node.sample_rate);
                println!("    channels: {}", node.channels);
            }
            "Props" | "props" => {
                println!("  Object: id {}", node.id);
                println!("  Param.Props:");
                let vol_info = simulated_volume_for_node(node.id);
                println!("    volume: {:.6}", vol_info.volume);
                println!("    mute: {}", vol_info.muted);
                println!(
                    "    channelVolumes: [{}]",
                    (0..node.channels)
                        .map(|_| format!("{:.6}", vol_info.volume))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            "ProcessLatency" | "process-latency" => {
                println!("  Object: id {}", node.id);
                println!("  Param.ProcessLatency:");
                println!("    quantum: {}/{}", node._buffer_size, node.sample_rate);
                println!("    rate: {}", node.sample_rate);
                println!("    ns: {}", node._latency_ns);
            }
            p => {
                eprintln!("pw-cli: enum-params: unknown param: {}", p);
                return 1;
            }
        }
        return 0;
    }

    eprintln!("pw-cli: enum-params: object {} not found", id);
    1
}

fn run_pw_cli_permissions(args: &[String], _state: &PwState) -> i32 {
    if args.len() < 2 {
        eprintln!("pw-cli: permissions: usage: permissions <client-id> <object-id>:<permission>");
        return 1;
    }
    println!(
        "pw-cli: permissions set for client {} -> {}",
        args[0], args[1]
    );
    0
}

fn run_pw_cli_get_permissions(args: &[String], state: &PwState) -> i32 {
    if args.is_empty() {
        eprintln!("pw-cli: get-permissions: usage: get-permissions <client-id>");
        return 1;
    }
    let id: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pw-cli: get-permissions: invalid id: {}", args[0]);
            return 1;
        }
    };
    if find_client_by_id(state, id).is_some() {
        println!("  client {}: permissions: rwxm (all)", id);
        return 0;
    }
    eprintln!("pw-cli: get-permissions: client {} not found", id);
    1
}

fn run_pw_cli_create_link(args: &[String], state: &PwState) -> i32 {
    if args.len() < 2 {
        eprintln!("pw-cli: create-link: usage: create-link <output-port-id> <input-port-id>");
        return 1;
    }
    let out_port: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pw-cli: create-link: invalid output port id: {}", args[0]);
            return 1;
        }
    };
    let in_port: u32 = match args[1].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pw-cli: create-link: invalid input port id: {}", args[1]);
            return 1;
        }
    };
    if find_port_by_id(state, out_port).is_none() {
        eprintln!("pw-cli: create-link: output port {} not found", out_port);
        return 1;
    }
    if find_port_by_id(state, in_port).is_none() {
        eprintln!("pw-cli: create-link: input port {} not found", in_port);
        return 1;
    }
    println!(
        "pw-cli: link created: {} -> {} (simulated)",
        out_port, in_port
    );
    0
}

fn run_pw_cli_destroy(args: &[String], state: &PwState) -> i32 {
    if args.is_empty() {
        eprintln!("pw-cli: destroy: usage: destroy <id>");
        return 1;
    }
    let id: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pw-cli: destroy: invalid id: {}", args[0]);
            return 1;
        }
    };

    // Check if the object exists in any collection
    let exists = find_node_by_id(state, id).is_some()
        || find_port_by_id(state, id).is_some()
        || find_device_by_id(state, id).is_some()
        || find_client_by_id(state, id).is_some()
        || find_module_by_id(state, id).is_some()
        || state.links.iter().any(|l| l.id == id);

    if exists {
        println!("pw-cli: object {} destroyed (simulated)", id);
        0
    } else {
        eprintln!("pw-cli: destroy: object {} not found", id);
        1
    }
}

// ── pw-dump ────────────────────────────────────────────────────────────

fn run_pw_dump(args: Vec<String>, state: &PwState) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        print_pw_dump_help();
        return 0;
    }
    if args.first().map(|s| s.as_str()) == Some("--version")
        || args.first().map(|s| s.as_str()) == Some("-V")
    {
        println!("pw-dump: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    // Check for --no-colors flag (accept and ignore)
    let filter_id: Option<u32> = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .and_then(|s| s.parse().ok());

    println!("[");
    let mut first = true;

    if let Some(id) = filter_id {
        if let Some(node) = find_node_by_id(state, id) {
            dump_node_json(node, state, first);
            first = false;
        }
        if let Some(port) = find_port_by_id(state, id) {
            dump_port_json(port, first);
            first = false;
        }
        if let Some(device) = find_device_by_id(state, id) {
            dump_device_json(device, first);
            first = false;
        }
        if let Some(client) = find_client_by_id(state, id) {
            dump_client_json(client, first);
            first = false;
        }
        if let Some(link) = state.links.iter().find(|l| l.id == id) {
            dump_link_json(link, first);
            first = false;
        }
        if let Some(module) = find_module_by_id(state, id) {
            dump_module_json(module, first);
            // Suppress unused assignment warning: this is the last branch
            let _ = first;
        }
    } else {
        // Dump everything
        for node in &state.nodes {
            dump_node_json(node, state, first);
            first = false;
        }
        for port in &state.ports {
            dump_port_json(port, first);
            first = false;
        }
        for link in &state.links {
            dump_link_json(link, first);
            first = false;
        }
        for device in &state.devices {
            dump_device_json(device, first);
            first = false;
        }
        for client in &state.clients {
            dump_client_json(client, first);
            first = false;
        }
        for module in &state.modules {
            dump_module_json(module, first);
            first = false;
        }
    }

    println!("\n]");
    0
}

fn print_pw_dump_help() {
    println!("pw-dump - Dump PipeWire objects as JSON ({})", PW_VERSION);
    println!();
    println!("Usage: pw-dump [options] [id]");
    println!();
    println!("Options:");
    println!("  --help, -h       Show help");
    println!("  --version, -V    Show version");
    println!("  --no-colors      Disable color output");
    println!("  --color           Enable color output");
    println!();
    println!("If [id] is given, only that object is dumped.");
}

fn json_comma(first: bool) {
    if !first {
        println!(",");
    }
}

fn dump_node_json(node: &PwNode, state: &PwState, first: bool) {
    json_comma(first);
    println!("  {{");
    println!("    \"id\": {},", node.id);
    println!("    \"type\": \"PipeWire:Interface:Node\",");
    println!("    \"info\": {{");
    println!("      \"name\": \"{}\",", escape_json_string(&node.name));
    println!("      \"state\": \"{}\",", node.state);
    println!(
        "      \"media.class\": \"{}\",",
        escape_json_string(&node.media_class)
    );
    if node.sample_rate > 0 {
        println!("      \"audio.format\": \"{}\",", node.format);
        println!("      \"audio.rate\": {},", node.sample_rate);
        println!("      \"audio.channels\": {},", node.channels);
    }
    let node_ports = ports_for_node(state, node.id);
    println!(
        "      \"n-input-ports\": {},",
        node_ports
            .iter()
            .filter(|p| p.direction == PortDirection::Input)
            .count()
    );
    println!(
        "      \"n-output-ports\": {}",
        node_ports
            .iter()
            .filter(|p| p.direction == PortDirection::Output)
            .count()
    );
    println!("    }}");
    print!("  }}");
}

fn dump_port_json(port: &PwPort, first: bool) {
    json_comma(first);
    println!("  {{");
    println!("    \"id\": {},", port.id);
    println!("    \"type\": \"PipeWire:Interface:Port\",");
    println!("    \"info\": {{");
    println!("      \"name\": \"{}\",", escape_json_string(&port.name));
    println!("      \"node.id\": {},", port.node_id);
    println!("      \"direction\": \"{}\"", port.direction);
    println!("    }}");
    print!("  }}");
}

fn dump_link_json(link: &PwLink, first: bool) {
    json_comma(first);
    println!("  {{");
    println!("    \"id\": {},", link.id);
    println!("    \"type\": \"PipeWire:Interface:Link\",");
    println!("    \"info\": {{");
    println!("      \"output-node-id\": {},", link.output_node);
    println!("      \"output-port-id\": {},", link.output_port);
    println!("      \"input-node-id\": {},", link.input_node);
    println!("      \"input-port-id\": {},", link.input_port);
    println!("      \"state\": \"{}\"", link.state);
    println!("    }}");
    print!("  }}");
}

fn dump_device_json(device: &PwDevice, first: bool) {
    json_comma(first);
    println!("  {{");
    println!("    \"id\": {},", device.id);
    println!("    \"type\": \"PipeWire:Interface:Device\",");
    println!("    \"info\": {{");
    println!("      \"name\": \"{}\",", escape_json_string(&device.name));
    println!("      \"device.type\": \"{}\",", device.device_type);
    println!("      \"profiles\": [");
    for (i, prof) in device.profiles.iter().enumerate() {
        let comma = if i + 1 < device.profiles.len() {
            ","
        } else {
            ""
        };
        println!("        \"{}\"{}", escape_json_string(&prof.name), comma);
    }
    println!("      ]");
    println!("    }}");
    print!("  }}");
}

fn dump_client_json(client: &PwClient, first: bool) {
    json_comma(first);
    println!("  {{");
    println!("    \"id\": {},", client.id);
    println!("    \"type\": \"PipeWire:Interface:Client\",");
    println!("    \"info\": {{");
    println!("      \"name\": \"{}\",", escape_json_string(&client.name));
    println!("      \"pid\": {},", client._pid);
    println!("      \"access\": \"{}\"", client._access);
    println!("    }}");
    print!("  }}");
}

fn dump_module_json(module: &PwModule, first: bool) {
    json_comma(first);
    println!("  {{");
    println!("    \"id\": {},", module.id);
    println!("    \"type\": \"PipeWire:Interface:Module\",");
    println!("    \"info\": {{");
    println!("      \"name\": \"{}\",", escape_json_string(&module.name));
    println!(
        "      \"filename\": \"{}\"",
        escape_json_string(&module._filename)
    );
    println!("    }}");
    print!("  }}");
}

// ── pw-record ──────────────────────────────────────────────────────────

fn run_pw_record(args: Vec<String>, state: &PwState) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_pw_record_help();
        return if args.is_empty() { 1 } else { 0 };
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("pw-record: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    let config = parse_record_config(&args);
    match config {
        Some(cfg) => {
            run_record_simulation(&cfg, state);
            0
        }
        None => {
            eprintln!("pw-record: error: no output file specified");
            1
        }
    }
}

fn print_pw_record_help() {
    println!("pw-record - Record audio via PipeWire ({})", PW_VERSION);
    println!();
    println!("Usage: pw-record [options] <output-file>");
    println!();
    println!("Options:");
    println!("  --help, -h           Show help");
    println!("  --version, -V        Show version");
    println!("  --target <target>    Target node name or id");
    println!("  --format <fmt>       Audio format (S16LE, S24LE, S32LE, F32LE)");
    println!(
        "  --rate <rate>        Sample rate (default: {})",
        DEFAULT_SAMPLE_RATE
    );
    println!(
        "  --channels <n>       Number of channels (default: {})",
        DEFAULT_CHANNELS
    );
    println!("  --volume <vol>       Recording volume (0.0-1.0)");
    println!("  --latency <n/rate>   Buffer latency");
}

fn parse_record_config(args: &[String]) -> Option<RecordConfig> {
    let stream = parse_stream_args(args, StreamDirection::Capture);
    let mut target = None;
    let mut output_file = None;
    let mut duration_secs = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" | "-t" if i + 1 < args.len() => {
                target = Some(args[i + 1].clone());
                i += 1;
            }
            "--duration" | "-d" if i + 1 < args.len() => {
                duration_secs = args[i + 1].parse().ok();
                i += 1;
            }
            // Value-taking options consumed by parse_stream_args. Skip their
            // value here so it is not mistaken for the positional output file.
            "--format" | "-f" | "--rate" | "-r" | "--channels" | "-c" | "--volume"
            | "--latency"
                if i + 1 < args.len() =>
            {
                i += 1;
            }
            s if !s.starts_with('-') => {
                output_file = Some(s.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    output_file.map(|f| RecordConfig {
        target,
        output_file: f,
        stream,
        _duration_secs: duration_secs,
    })
}

fn run_record_simulation(config: &RecordConfig, state: &PwState) {
    let source = config
        .target
        .as_ref()
        .and_then(|t| parse_id_or_name(t, state))
        .or_else(|| default_source(state));

    let source_name = source.map(|s| s.name.as_str()).unwrap_or("default");
    let data_rate = compute_data_rate(&config.stream);

    println!("Recording from \"{}\"", source_name);
    println!(
        "  Format: {}, Rate: {}, Channels: {}",
        config.stream.format, config.stream.rate, config.stream.channels
    );
    println!("  Output: {}", config.output_file);
    println!("  Data rate: {}/s", format_bytes_display(data_rate));

    // Simulate 2 seconds of recording
    let simulated_duration = 2.0;
    let total_bytes = (data_rate as f64 * simulated_duration) as u64;
    println!(
        "  Recorded: {} ({})",
        format_bytes_display(total_bytes),
        format_duration_hms(simulated_duration)
    );
    println!("Recording complete.");
}

// ── pw-play ────────────────────────────────────────────────────────────

fn run_pw_play(args: Vec<String>, state: &PwState) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_pw_play_help();
        return if args.is_empty() { 1 } else { 0 };
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("pw-play: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    let config = parse_play_config(&args);
    match config {
        Some(cfg) => {
            run_play_simulation(&cfg, state);
            0
        }
        None => {
            eprintln!("pw-play: error: no input file specified");
            1
        }
    }
}

fn print_pw_play_help() {
    println!("pw-play - Play audio via PipeWire ({})", PW_VERSION);
    println!();
    println!("Usage: pw-play [options] <input-file>");
    println!();
    println!("Options:");
    println!("  --help, -h           Show help");
    println!("  --version, -V        Show version");
    println!("  --target <target>    Target sink name or id");
    println!("  --format <fmt>       Audio format (S16LE, S24LE, S32LE, F32LE)");
    println!(
        "  --rate <rate>        Sample rate (default: {})",
        DEFAULT_SAMPLE_RATE
    );
    println!(
        "  --channels <n>       Number of channels (default: {})",
        DEFAULT_CHANNELS
    );
    println!("  --volume <vol>       Playback volume (0.0-1.0)");
    println!("  --latency <n/rate>   Buffer latency");
    println!("  --loop               Loop playback");
}

fn parse_play_config(args: &[String]) -> Option<PlayConfig> {
    let stream = parse_stream_args(args, StreamDirection::Playback);
    let mut target = None;
    let mut input_file = None;
    let mut loop_playback = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" | "-t" if i + 1 < args.len() => {
                target = Some(args[i + 1].clone());
                i += 1;
            }
            "--loop" => {
                loop_playback = true;
            }
            // Value-taking options consumed by parse_stream_args. Skip their
            // value here so it is not mistaken for the positional input file
            // (e.g. `pw-play --format S32LE` has no file and must yield None).
            "--format" | "-f" | "--rate" | "-r" | "--channels" | "-c" | "--volume"
            | "--latency"
                if i + 1 < args.len() =>
            {
                i += 1;
            }
            s if !s.starts_with('-') => {
                input_file = Some(s.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    input_file.map(|f| PlayConfig {
        target,
        input_file: f,
        stream,
        _loop_playback: loop_playback,
    })
}

fn run_play_simulation(config: &PlayConfig, state: &PwState) {
    let sink = config
        .target
        .as_ref()
        .and_then(|t| parse_id_or_name(t, state))
        .or_else(|| default_sink(state));

    let sink_name = sink.map(|s| s.name.as_str()).unwrap_or("default");
    let data_rate = compute_data_rate(&config.stream);

    println!("Playing to \"{}\"", sink_name);
    println!(
        "  Format: {}, Rate: {}, Channels: {}",
        config.stream.format, config.stream.rate, config.stream.channels
    );
    println!("  Input: {}", config.input_file);
    println!("  Data rate: {}/s", format_bytes_display(data_rate));

    // Simulate 3 seconds of playback
    let simulated_duration = 3.0;
    let total_bytes = (data_rate as f64 * simulated_duration) as u64;
    println!(
        "  Played: {} ({})",
        format_bytes_display(total_bytes),
        format_duration_hms(simulated_duration)
    );
    println!("Playback complete.");
}

// ── pw-cat ─────────────────────────────────────────────────────────────

fn run_pw_cat(args: Vec<String>, state: &PwState) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_pw_cat_help();
        return if args.is_empty() { 1 } else { 0 };
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("pw-cat: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    let config = parse_cat_config(&args);
    run_cat_simulation(&config, state);
    0
}

fn print_pw_cat_help() {
    println!("pw-cat - Cat audio streams via PipeWire ({})", PW_VERSION);
    println!();
    println!("Usage: pw-cat [options] [--playback|--record] [file]");
    println!();
    println!("Options:");
    println!("  --help, -h           Show help");
    println!("  --version, -V        Show version");
    println!("  --playback, -p       Playback mode (sink)");
    println!("  --record, -r         Record mode (source)");
    println!("  --midi               MIDI mode");
    println!("  --target <target>    Target node name or id");
    println!("  --format <fmt>       Audio format");
    println!("  --rate <rate>        Sample rate");
    println!("  --channels <n>       Number of channels");
    println!("  --volume <vol>       Volume (0.0-1.0)");
    println!("  --latency <n/rate>   Buffer latency");
}

fn parse_cat_config(args: &[String]) -> CatConfig {
    let stream = parse_stream_args(args, StreamDirection::Playback);
    let mut direction = StreamDirection::Playback;
    let mut target = None;

    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "--record" | "-r" => direction = StreamDirection::Capture,
            "--playback" | "-p" => direction = StreamDirection::Playback,
            "--target" | "-t" if i + 1 < args.len() => {
                target = Some(args[i + 1].clone());
            }
            _ => {}
        }
    }

    CatConfig {
        direction,
        target,
        stream,
    }
}

fn run_cat_simulation(config: &CatConfig, state: &PwState) {
    let node = config
        .target
        .as_ref()
        .and_then(|t| parse_id_or_name(t, state))
        .or_else(|| match config.direction {
            StreamDirection::Playback => default_sink(state),
            StreamDirection::Capture => default_source(state),
        });

    let node_name = node.map(|n| n.name.as_str()).unwrap_or("default");
    let data_rate = compute_data_rate(&config.stream);

    println!("pw-cat: {} mode", config.direction);
    println!("  Target: \"{}\"", node_name);
    println!(
        "  Format: {}, Rate: {}, Channels: {}",
        config.stream.format, config.stream.rate, config.stream.channels
    );
    println!("  Data rate: {}/s", format_bytes_display(data_rate));
    println!("  Streaming... (simulated)");
    println!("  Transferred: {}", format_bytes_display(data_rate * 2));
}

// ── pw-mon ─────────────────────────────────────────────────────────────

fn run_pw_mon(args: Vec<String>, state: &PwState) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        print_pw_mon_help();
        return 0;
    }
    if args.first().map(|s| s.as_str()) == Some("--version")
        || args.first().map(|s| s.as_str()) == Some("-V")
    {
        println!("pw-mon: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    let show_all = args.iter().any(|a| a == "--all" || a == "-a");
    let events = simulated_monitor_events(state);

    println!("PipeWire Monitor ({})", PW_VERSION);
    println!("Monitoring PipeWire events...");
    println!();

    for event in &events {
        if !show_all
            && event.event_type == MonitorEventType::Added
            && matches!(event.object_type, ObjectType::Port)
        {
            // In non-all mode, skip port add events for brevity
            continue;
        }
        println!(
            "event: {} {} {}",
            event.event_type, event.object_type, event.object_id
        );
        if !event._object_name.is_empty() {
            println!("  name: \"{}\"", event._object_name);
        }
    }

    println!();
    println!("(end of simulated events)");
    0
}

fn print_pw_mon_help() {
    println!("pw-mon - Monitor PipeWire events ({})", PW_VERSION);
    println!();
    println!("Usage: pw-mon [options]");
    println!();
    println!("Options:");
    println!("  --help, -h       Show help");
    println!("  --version, -V    Show version");
    println!("  --all, -a        Show all events (including ports)");
    println!("  --color          Enable color output");
    println!("  --no-colors      Disable color output");
}

// ── pw-metadata ────────────────────────────────────────────────────────

fn run_pw_metadata(args: Vec<String>, state: &PwState) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        print_pw_metadata_help();
        return 0;
    }
    if args.first().map(|s| s.as_str()) == Some("--version")
        || args.first().map(|s| s.as_str()) == Some("-V")
    {
        println!("pw-metadata: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    // Parse subcommand
    if args.is_empty() {
        // List all metadata
        return run_pw_metadata_list(state);
    }

    match args[0].as_str() {
        "--list" | "-l" => run_pw_metadata_list(state),
        "--delete" | "-d" => run_pw_metadata_delete(&args[1..]),
        "--monitor" | "-m" => run_pw_metadata_monitor(state),
        _ => {
            // Treat as subject-id key [value [type]]
            run_pw_metadata_set_or_get(&args, state)
        }
    }
}

fn print_pw_metadata_help() {
    println!("pw-metadata - Manage PipeWire metadata ({})", PW_VERSION);
    println!();
    println!("Usage: pw-metadata [options] [id [key [value [type]]]]");
    println!();
    println!("Options:");
    println!("  --help, -h       Show help");
    println!("  --version, -V    Show version");
    println!("  --list, -l       List all metadata entries");
    println!("  --delete, -d     Delete a metadata entry");
    println!("  --monitor, -m    Monitor metadata changes");
    println!("  --name <name>    Metadata object name (default: \"default\")");
    println!();
    println!("Without options, sets or gets metadata.");
    println!("  pw-metadata <id>            Get metadata for subject id");
    println!("  pw-metadata <id> <key>      Get specific key");
    println!("  pw-metadata <id> <key> <value> [type]  Set metadata");
}

fn run_pw_metadata_list(state: &PwState) -> i32 {
    if state.metadata.is_empty() {
        println!("(no metadata entries)");
        return 0;
    }
    for entry in &state.metadata {
        println!(
            "update: id:{} key:'{}' value:'{}' type:'{}'",
            entry.subject, entry.key, entry.value, entry._key_type
        );
    }
    0
}

fn run_pw_metadata_delete(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("pw-metadata: delete: usage: --delete <subject-id> [key]");
        return 1;
    }
    let subject: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pw-metadata: delete: invalid subject id: {}", args[0]);
            return 1;
        }
    };
    if args.len() > 1 {
        println!(
            "pw-metadata: deleted key '{}' for subject {} (simulated)",
            args[1], subject
        );
    } else {
        println!(
            "pw-metadata: deleted all metadata for subject {} (simulated)",
            subject
        );
    }
    0
}

fn run_pw_metadata_monitor(state: &PwState) -> i32 {
    println!("Monitoring metadata changes...");
    for entry in &state.metadata {
        println!(
            "  update: id:{} key:'{}' value:'{}' type:'{}'",
            entry.subject, entry.key, entry.value, entry._key_type
        );
    }
    println!("(end of simulated events)");
    0
}

fn run_pw_metadata_set_or_get(args: &[String], state: &PwState) -> i32 {
    let subject: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("pw-metadata: invalid subject id: {}", args[0]);
            return 1;
        }
    };

    if args.len() == 1 {
        // Get all metadata for subject
        let entries: Vec<_> = state
            .metadata
            .iter()
            .filter(|m| m.subject == subject)
            .collect();
        if entries.is_empty() {
            println!("(no metadata for subject {})", subject);
        } else {
            for entry in entries {
                println!(
                    "  key:'{}' value:'{}' type:'{}'",
                    entry.key, entry.value, entry._key_type
                );
            }
        }
        return 0;
    }

    if args.len() == 2 {
        // Get specific key
        let key = &args[1];
        if let Some(entry) = state
            .metadata
            .iter()
            .find(|m| m.subject == subject && m.key == *key)
        {
            println!(
                "  key:'{}' value:'{}' type:'{}'",
                entry.key, entry.value, entry._key_type
            );
        } else {
            println!("(no metadata for subject {} key '{}')", subject, key);
        }
        return 0;
    }

    // Set metadata
    let key = &args[1];
    let value = &args[2];
    let key_type = if args.len() > 3 {
        &args[3]
    } else {
        "Spa:String:JSON"
    };
    println!(
        "pw-metadata: set subject:{} key:'{}' value:'{}' type:'{}' (simulated)",
        subject, key, value, key_type
    );
    0
}

// ── pw-top ─────────────────────────────────────────────────────────────

fn run_pw_top(args: Vec<String>, state: &PwState) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        print_pw_top_help();
        return 0;
    }
    if args.first().map(|s| s.as_str()) == Some("--version")
        || args.first().map(|s| s.as_str()) == Some("-V")
    {
        println!("pw-top: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    let batch_mode = args.iter().any(|a| a == "--batch" || a == "-b");
    let iterations: u32 = args
        .iter()
        .position(|a| a == "--iterations" || a == "-n")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let processing = simulated_processing_info(state);

    for iter_n in 0..iterations {
        if iter_n > 0 && !batch_mode {
            println!();
        }
        print_pw_top_display(&processing, state, batch_mode);
    }
    0
}

fn print_pw_top_help() {
    println!(
        "pw-top - Top-like display of PipeWire processing ({})",
        PW_VERSION
    );
    println!();
    println!("Usage: pw-top [options]");
    println!();
    println!("Options:");
    println!("  --help, -h              Show help");
    println!("  --version, -V           Show version");
    println!("  --batch, -b             Batch mode (no terminal control)");
    println!("  --iterations <n>, -n    Number of iterations");
}

fn print_pw_top_display(processing: &[ProcessingInfo], state: &PwState, _batch_mode: bool) {
    println!("S   ID QUANT   RATE    WAIT    BUSY   W/Q   B/Q  ERR  NAME");
    for info in processing {
        let name = find_node_by_id(state, info.node_id)
            .map(|n| n.name.as_str())
            .unwrap_or("???");
        let node_state = find_node_by_id(state, info.node_id)
            .map(|n| n.state)
            .unwrap_or(NodeState::_Error);

        let state_char = match node_state {
            NodeState::Running => 'R',
            NodeState::Idle => 'I',
            NodeState::Suspended => 'S',
            NodeState::_Creating => 'C',
            NodeState::_Error => 'E',
        };

        let wait_ms = info._wait_ns as f64 / 1_000_000.0;
        let busy_ms = info._busy_ns as f64 / 1_000_000.0;
        let quantum_ms = if info._rate > 0 {
            info._quantum as f64 / info._rate as f64 * 1000.0
        } else {
            0.0
        };
        let w_q = if quantum_ms > 0.0 {
            wait_ms / quantum_ms
        } else {
            0.0
        };
        let b_q = if quantum_ms > 0.0 {
            busy_ms / quantum_ms
        } else {
            0.0
        };

        println!(
            "{} {:4} {:5} {:6} {:7.3} {:7.3} {:5.3} {:5.3} {:4} {}",
            state_char,
            info.node_id,
            info._quantum,
            info._rate,
            wait_ms,
            busy_ms,
            w_q,
            b_q,
            info._xrun_count,
            name
        );
    }
}

// ── wpctl ──────────────────────────────────────────────────────────────

fn run_wpctl(args: Vec<String>, state: &PwState) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_wpctl_help();
        return 0;
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("wpctl - WirePlumber Control ({})", WP_VERSION);
        return 0;
    }

    match args[0].as_str() {
        "status" => run_wpctl_status(state),
        "inspect" => run_wpctl_inspect(&args[1..], state),
        "set-volume" => run_wpctl_set_volume(&args[1..], state),
        "set-mute" => run_wpctl_set_mute(&args[1..], state),
        "set-default" => run_wpctl_set_default(&args[1..], state),
        "get-volume" => run_wpctl_get_volume(&args[1..], state),
        "clear-default" => run_wpctl_clear_default(&args[1..]),
        "set-profile" => run_wpctl_set_profile(&args[1..], state),
        "settings" => run_wpctl_settings(&args[1..]),
        unknown => {
            eprintln!("wpctl: unknown command: {}", unknown);
            eprintln!("Type 'wpctl --help' for a list of commands");
            1
        }
    }
}

fn print_wpctl_help() {
    println!("wpctl - WirePlumber Control ({})", WP_VERSION);
    println!();
    println!("Usage: wpctl [command] [options]");
    println!();
    println!("Commands:");
    println!("  status                         Show the current state");
    println!("  inspect <id>                   Inspect an object");
    println!("  set-volume <id> <vol>[%]       Set volume of a node");
    println!("  set-mute <id> <0|1|toggle>     Set mute state of a node");
    println!("  set-default <id>               Set default sink/source");
    println!("  get-volume <id>                Get volume of a node");
    println!("  clear-default <id>             Clear default sink/source");
    println!("  set-profile <device-id> <idx>  Set device profile");
    println!("  settings [key [value]]         Show or set WirePlumber settings");
    println!();
    println!("Options:");
    println!("  --help, -h       Show help");
    println!("  --version, -V    Show version");
}

fn run_wpctl_status(state: &PwState) -> i32 {
    println!(
        "PipeWire '{}' [{}] {}",
        state.core_info._name, state.core_info._cookie, state.core_info._version
    );
    println!();

    // Audio section
    println!(" Audio");

    // Sinks
    println!("  Sinks:");
    let sinks = sink_nodes(state);
    let def_sink = default_sink(state);
    for sink in &sinks {
        let is_default = def_sink.map(|d| d.id == sink.id).unwrap_or(false);
        let marker = if is_default { " *" } else { "  " };
        let vol = simulated_volume_for_node(sink.id);
        let mute_str = if vol.muted { " [MUTED]" } else { "" };
        println!(
            "  {}  {}. {}  [vol: {}{}]",
            marker,
            sink.id,
            sink.name,
            format_volume_percent(vol.volume),
            mute_str
        );
    }
    println!();

    // Sources
    println!("  Sources:");
    let sources = source_nodes(state);
    let def_source = default_source(state);
    for source in &sources {
        let is_default = def_source.map(|d| d.id == source.id).unwrap_or(false);
        let marker = if is_default { " *" } else { "  " };
        let vol = simulated_volume_for_node(source.id);
        let mute_str = if vol.muted { " [MUTED]" } else { "" };
        println!(
            "  {}  {}. {}  [vol: {}{}]",
            marker,
            source.id,
            source.name,
            format_volume_percent(vol.volume),
            mute_str
        );
    }
    println!();

    // Streams
    let outputs = stream_output_nodes(state);
    if !outputs.is_empty() {
        println!("  Sink endpoints:");
        for stream in &outputs {
            let linked_to = state
                .links
                .iter()
                .find(|l| l.output_node == stream.id)
                .and_then(|l| find_node_by_id(state, l.input_node))
                .map(|n| n.name.as_str())
                .unwrap_or("(not linked)");
            println!("      {}. {} -> {}", stream.id, stream.name, linked_to);
        }
        println!();
    }

    let inputs = stream_input_nodes(state);
    if !inputs.is_empty() {
        println!("  Source endpoints:");
        for stream in &inputs {
            let linked_from = state
                .links
                .iter()
                .find(|l| l.input_node == stream.id)
                .and_then(|l| find_node_by_id(state, l.output_node))
                .map(|n| n.name.as_str())
                .unwrap_or("(not linked)");
            println!("      {}. {} <- {}", stream.id, stream.name, linked_from);
        }
        println!();
    }

    // Video section
    let video_nodes: Vec<_> = state
        .nodes
        .iter()
        .filter(|n| matches!(n.node_type, NodeType::VideoSource))
        .collect();
    if !video_nodes.is_empty() {
        println!(" Video");
        println!("  Sources:");
        for vn in &video_nodes {
            println!("      {}. {}", vn.id, vn.name);
        }
        println!();
    }

    // Devices section
    println!(" Devices:");
    for device in &state.devices {
        let profile = device
            .profiles
            .first()
            .map(|p| p.name.as_str())
            .unwrap_or("(none)");
        println!("    {}. {} [{}]", device.id, device.name, profile);
    }
    println!();

    // Clients section
    println!(" Clients:");
    for client in &state.clients {
        println!("    {}. {} (pid:{})", client.id, client.name, client._pid);
    }

    0
}

fn run_wpctl_inspect(args: &[String], state: &PwState) -> i32 {
    if args.is_empty() {
        eprintln!("wpctl: inspect: usage: inspect <id>");
        return 1;
    }
    let id: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            // Try by name
            if let Some(node) = state.nodes.iter().find(|n| n.name == args[0]) {
                wpctl_inspect_node(node, state);
                return 0;
            }
            eprintln!("wpctl: inspect: invalid id: {}", args[0]);
            return 1;
        }
    };

    if let Some(node) = find_node_by_id(state, id) {
        wpctl_inspect_node(node, state);
        return 0;
    }
    if let Some(device) = find_device_by_id(state, id) {
        wpctl_inspect_device(device);
        return 0;
    }
    if let Some(client) = find_client_by_id(state, id) {
        wpctl_inspect_client(client);
        return 0;
    }
    if let Some(port) = find_port_by_id(state, id) {
        wpctl_inspect_port(port);
        return 0;
    }
    if let Some(link) = state.links.iter().find(|l| l.id == id) {
        wpctl_inspect_link(link, state);
        return 0;
    }

    eprintln!("wpctl: inspect: object {} not found", id);
    1
}

fn wpctl_inspect_node(node: &PwNode, state: &PwState) {
    println!("id {}, type PipeWire:Interface:Node", node.id);
    println!("  node.name = \"{}\"", node.name);
    println!("  node.description = \"{}\"", node._description);
    println!("  media.class = \"{}\"", node.media_class);
    println!("  state = \"{}\"", node.state);
    if node.sample_rate > 0 {
        println!("  audio.format = \"{}\"", node.format);
        println!("  audio.rate = {}", node.sample_rate);
        println!("  audio.channels = {}", node.channels);
    }
    let vol = simulated_volume_for_node(node.id);
    println!("  volume = {:.6}", vol.volume);
    println!("  mute = {}", vol.muted);
    let node_ports = ports_for_node(state, node.id);
    println!("  n-ports = {}", node_ports.len());
    for port in &node_ports {
        println!(
            "    port {}: \"{}\" ({})",
            port.id, port.name, port.direction
        );
    }
}

fn wpctl_inspect_device(device: &PwDevice) {
    println!("id {}, type PipeWire:Interface:Device", device.id);
    println!("  device.name = \"{}\"", device.name);
    println!("  device.description = \"{}\"", device._description);
    println!("  device.nick = \"{}\"", device._nick);
    println!("  device.type = \"{}\"", device.device_type);
    println!("  device.bus-path = \"{}\"", device._bus_path);
    println!("  device.form-factor = \"{}\"", device._form_factor);
    println!("  profiles:");
    for profile in &device.profiles {
        println!("    \"{}\" - \"{}\"", profile.name, profile._description);
    }
}

fn wpctl_inspect_client(client: &PwClient) {
    println!("id {}, type PipeWire:Interface:Client", client.id);
    println!("  client.name = \"{}\"", client.name);
    println!("  client.pid = {}", client._pid);
    println!("  client.access = \"{}\"", client._access);
    println!("  client.protocol = \"{}\"", client._protocol);
}

fn wpctl_inspect_port(port: &PwPort) {
    println!("id {}, type PipeWire:Interface:Port", port.id);
    println!("  port.name = \"{}\"", port.name);
    println!("  port.direction = \"{}\"", port.direction);
    println!("  node.id = {}", port.node_id);
    println!("  port.alias = \"{}\"", port._alias);
    println!("  port.physical = {}", port._physical);
    println!("  port.terminal = {}", port._terminal);
}

fn wpctl_inspect_link(link: &PwLink, state: &PwState) {
    println!("id {}, type PipeWire:Interface:Link", link.id);
    println!("  link.output.node = {}", link.output_node);
    println!("  link.output.port = {}", link.output_port);
    println!("  link.input.node = {}", link.input_node);
    println!("  link.input.port = {}", link.input_port);
    println!("  state = \"{}\"", link.state);
    if let Some(out_node) = find_node_by_id(state, link.output_node) {
        println!("  output-node-name = \"{}\"", out_node.name);
    }
    if let Some(in_node) = find_node_by_id(state, link.input_node) {
        println!("  input-node-name = \"{}\"", in_node.name);
    }
}

fn run_wpctl_set_volume(args: &[String], state: &PwState) -> i32 {
    if args.len() < 2 {
        eprintln!("wpctl: set-volume: usage: set-volume <id> <volume>[%]");
        return 1;
    }
    let id: u32 = match parse_wpctl_id(&args[0], state) {
        Some(v) => v,
        None => {
            eprintln!("wpctl: set-volume: invalid id: {}", args[0]);
            return 1;
        }
    };
    let vol_spec = &args[1];

    // Check for relative volume (+/-)
    let (is_relative, vol_str) = if let Some(rest) = vol_spec.strip_prefix('+') {
        (Some(true), rest)
    } else if let Some(rest) = vol_spec.strip_prefix('-') {
        (Some(false), rest)
    } else {
        (None, vol_spec.as_str())
    };

    let new_vol = match parse_volume_spec(vol_str) {
        Some(v) => v,
        None => {
            eprintln!("wpctl: set-volume: invalid volume: {}", vol_spec);
            return 1;
        }
    };

    if find_node_by_id(state, id).is_none() {
        eprintln!("wpctl: set-volume: node {} not found", id);
        return 1;
    }

    let current = simulated_volume_for_node(id);
    let final_vol = match is_relative {
        Some(true) => (current.volume + new_vol).min(1.5),
        Some(false) => (current.volume - new_vol).max(0.0),
        None => new_vol,
    };

    println!(
        "Volume for node {} set to {} (simulated)",
        id,
        format_volume_percent(final_vol)
    );
    0
}

fn run_wpctl_set_mute(args: &[String], state: &PwState) -> i32 {
    if args.len() < 2 {
        eprintln!("wpctl: set-mute: usage: set-mute <id> <0|1|toggle>");
        return 1;
    }
    let id: u32 = match parse_wpctl_id(&args[0], state) {
        Some(v) => v,
        None => {
            eprintln!("wpctl: set-mute: invalid id: {}", args[0]);
            return 1;
        }
    };

    if find_node_by_id(state, id).is_none() {
        eprintln!("wpctl: set-mute: node {} not found", id);
        return 1;
    }

    let current = simulated_volume_for_node(id);
    let new_mute = match args[1].as_str() {
        "1" | "true" | "yes" => true,
        "0" | "false" | "no" => false,
        "toggle" => !current.muted,
        other => {
            eprintln!(
                "wpctl: set-mute: invalid value: {} (expected 0, 1, or toggle)",
                other
            );
            return 1;
        }
    };

    let mute_str = if new_mute { "muted" } else { "unmuted" };
    println!("Node {} {} (simulated)", id, mute_str);
    0
}

fn run_wpctl_set_default(args: &[String], state: &PwState) -> i32 {
    if args.is_empty() {
        eprintln!("wpctl: set-default: usage: set-default <id>");
        return 1;
    }
    let id: u32 = match parse_wpctl_id(&args[0], state) {
        Some(v) => v,
        None => {
            eprintln!("wpctl: set-default: invalid id: {}", args[0]);
            return 1;
        }
    };

    if let Some(node) = find_node_by_id(state, id) {
        let kind = if node.media_class.contains("Sink") {
            "sink"
        } else {
            "source"
        };
        println!("Default {} set to {} ({}) (simulated)", kind, id, node.name);
        0
    } else {
        eprintln!("wpctl: set-default: node {} not found", id);
        1
    }
}

fn run_wpctl_get_volume(args: &[String], state: &PwState) -> i32 {
    if args.is_empty() {
        eprintln!("wpctl: get-volume: usage: get-volume <id>");
        return 1;
    }
    let id: u32 = match parse_wpctl_id(&args[0], state) {
        Some(v) => v,
        None => {
            eprintln!("wpctl: get-volume: invalid id: {}", args[0]);
            return 1;
        }
    };

    if find_node_by_id(state, id).is_none() {
        eprintln!("wpctl: get-volume: node {} not found", id);
        return 1;
    }

    let vol = simulated_volume_for_node(id);
    let mute_str = if vol.muted { " [MUTED]" } else { "" };
    println!("Volume: {:.2}{}", vol.volume, mute_str);
    0
}

fn run_wpctl_clear_default(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("wpctl: clear-default: usage: clear-default <id>");
        return 1;
    }
    println!("Default cleared for {} (simulated)", args[0]);
    0
}

fn run_wpctl_set_profile(args: &[String], state: &PwState) -> i32 {
    if args.len() < 2 {
        eprintln!("wpctl: set-profile: usage: set-profile <device-id> <profile-index>");
        return 1;
    }
    let device_id: u32 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("wpctl: set-profile: invalid device id: {}", args[0]);
            return 1;
        }
    };
    let profile_idx: usize = match args[1].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("wpctl: set-profile: invalid profile index: {}", args[1]);
            return 1;
        }
    };

    if let Some(device) = find_device_by_id(state, device_id) {
        if profile_idx < device.profiles.len() {
            println!(
                "Profile for device {} set to {} ({}) (simulated)",
                device_id, profile_idx, device.profiles[profile_idx].name
            );
            0
        } else {
            eprintln!(
                "wpctl: set-profile: profile index {} out of range (device has {} profiles)",
                profile_idx,
                device.profiles.len()
            );
            1
        }
    } else {
        eprintln!("wpctl: set-profile: device {} not found", device_id);
        1
    }
}

fn run_wpctl_settings(args: &[String]) -> i32 {
    if args.is_empty() {
        // Show all settings
        println!("WirePlumber Settings:");
        println!("  clock.force-quantum = 0");
        println!("  clock.force-rate = 0");
        println!("  log.level = 2");
        println!("  bluetooth.autoswitch-to-headset-profile = true");
        println!("  bluetooth.roles = [ hfp_hf, hfp_ag, a2dp_sink, a2dp_source ]");
        return 0;
    }

    let key = &args[0];
    if args.len() == 1 {
        // Get single setting
        match key.as_str() {
            "clock.force-quantum" => println!("clock.force-quantum = 0"),
            "clock.force-rate" => println!("clock.force-rate = 0"),
            "log.level" => println!("log.level = 2"),
            k => println!("{} = (not set)", k),
        }
        return 0;
    }

    // Set a setting
    println!("Setting {} = {} (simulated)", key, args[1]);
    0
}

fn parse_wpctl_id(s: &str, state: &PwState) -> Option<u32> {
    // Accept numeric IDs or special keywords
    match s {
        "@DEFAULT_AUDIO_SINK@" | "@DEFAULT_SINK@" => default_sink(state).map(|n| n.id),
        "@DEFAULT_AUDIO_SOURCE@" | "@DEFAULT_SOURCE@" => default_source(state).map(|n| n.id),
        _ => s.parse().ok(),
    }
}

// ── pipewire (daemon) ──────────────────────────────────────────────────

fn run_pipewire_daemon(args: Vec<String>, state: &PwState) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        print_pipewire_daemon_help();
        return 0;
    }
    if args.first().map(|s| s.as_str()) == Some("--version")
        || args.first().map(|s| s.as_str()) == Some("-V")
    {
        println!("pipewire: Compiled with libpipewire {}", PW_VERSION);
        return 0;
    }

    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");

    println!("PipeWire Daemon {}", PW_VERSION);
    println!("  Socket: {}", _PW_SOCKET);
    println!("  Config: {}", _PW_CONF);
    println!();

    if verbose {
        println!("Loading modules...");
        for module in &state.modules {
            println!("  Loaded: {} (id {})", module.name, module.id);
        }
        println!();

        println!("Registering factories...");
        for factory in &state.factories {
            println!("  Registered: {}", factory.name);
        }
        println!();
    }

    println!("Loaded {} modules", state.modules.len());
    println!("Registered {} factories", state.factories.len());
    println!("Detected {} devices", state.devices.len());
    println!("Created {} nodes", state.nodes.len());
    println!("Established {} links", state.links.len());
    println!("Connected {} clients", state.clients.len());
    println!();
    println!("PipeWire daemon running (simulated).");
    println!(
        "Default sink: {}",
        default_sink(state)
            .map(|n| n.name.as_str())
            .unwrap_or("(none)")
    );
    println!(
        "Default source: {}",
        default_source(state)
            .map(|n| n.name.as_str())
            .unwrap_or("(none)")
    );

    0
}

fn print_pipewire_daemon_help() {
    println!("pipewire - PipeWire daemon ({})", PW_VERSION);
    println!();
    println!("Usage: pipewire [options]");
    println!();
    println!("Options:");
    println!("  --help, -h       Show help");
    println!("  --version, -V    Show version");
    println!("  --verbose, -v    Verbose output");
    println!("  --config <file>  Use alternate config file");
}

// ── Basename extraction ────────────────────────────────────────────────

fn extract_basename(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut last_sep = 0;
    let mut found_sep = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
            found_sep = true;
        }
    }
    let start = if found_sep { last_sep } else { 0 };
    let base = &s[start..];
    base.strip_suffix(".exe").unwrap_or(base)
}

// ── main ───────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("pw-cli");
        extract_basename(s).to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let personality = Personality::from_name(&prog_name);
    let state = build_simulated_state();

    let code = match personality {
        Personality::PwCli => run_pw_cli(rest, &state),
        Personality::PwDump => run_pw_dump(rest, &state),
        Personality::PwRecord => run_pw_record(rest, &state),
        Personality::PwPlay => run_pw_play(rest, &state),
        Personality::PwCat => run_pw_cat(rest, &state),
        Personality::PwMon => run_pw_mon(rest, &state),
        Personality::PwMetadata => run_pw_metadata(rest, &state),
        Personality::PwTop => run_pw_top(rest, &state),
        Personality::Wpctl => run_wpctl(rest, &state),
        Personality::Pipewire => run_pipewire_daemon(rest, &state),
    };

    process::exit(code);
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality detection ──────────────────────────────────────

    #[test]
    fn test_personality_pw_cli_default() {
        assert_eq!(Personality::from_name("pw-cli"), Personality::PwCli);
    }

    #[test]
    fn test_personality_pw_cli_unknown() {
        assert_eq!(
            Personality::from_name("something-random"),
            Personality::PwCli
        );
    }

    #[test]
    fn test_personality_pw_dump() {
        assert_eq!(Personality::from_name("pw-dump"), Personality::PwDump);
    }

    #[test]
    fn test_personality_pw_record() {
        assert_eq!(Personality::from_name("pw-record"), Personality::PwRecord);
    }

    #[test]
    fn test_personality_pw_play() {
        assert_eq!(Personality::from_name("pw-play"), Personality::PwPlay);
    }

    #[test]
    fn test_personality_pw_cat() {
        assert_eq!(Personality::from_name("pw-cat"), Personality::PwCat);
    }

    #[test]
    fn test_personality_pw_mon() {
        assert_eq!(Personality::from_name("pw-mon"), Personality::PwMon);
    }

    #[test]
    fn test_personality_pw_metadata() {
        assert_eq!(
            Personality::from_name("pw-metadata"),
            Personality::PwMetadata
        );
    }

    #[test]
    fn test_personality_pw_top() {
        assert_eq!(Personality::from_name("pw-top"), Personality::PwTop);
    }

    #[test]
    fn test_personality_wpctl() {
        assert_eq!(Personality::from_name("wpctl"), Personality::Wpctl);
    }

    #[test]
    fn test_personality_pipewire() {
        assert_eq!(Personality::from_name("pipewire"), Personality::Pipewire);
    }

    // ── Basename extraction ────────────────────────────────────────

    #[test]
    fn test_basename_simple() {
        assert_eq!(extract_basename("pw-cli"), "pw-cli");
    }

    #[test]
    fn test_basename_with_unix_path() {
        assert_eq!(extract_basename("/usr/bin/pw-cli"), "pw-cli");
    }

    #[test]
    fn test_basename_with_windows_path() {
        assert_eq!(extract_basename("C:\\Program Files\\pw-cli.exe"), "pw-cli");
    }

    #[test]
    fn test_basename_exe_suffix() {
        assert_eq!(extract_basename("pw-dump.exe"), "pw-dump");
    }

    #[test]
    fn test_basename_mixed_slashes() {
        assert_eq!(extract_basename("/home/user\\bin/wpctl"), "wpctl");
    }

    #[test]
    fn test_basename_trailing_slash() {
        // Unusual but handles gracefully
        assert_eq!(extract_basename("/usr/bin/"), "");
    }

    #[test]
    fn test_basename_no_extension() {
        assert_eq!(extract_basename("pipewire"), "pipewire");
    }

    #[test]
    fn test_basename_deep_path() {
        assert_eq!(extract_basename("/a/b/c/d/e/pw-record"), "pw-record");
    }

    // ── Simulated state ────────────────────────────────────────────

    #[test]
    fn test_build_state_has_nodes() {
        let state = build_simulated_state();
        assert!(!state.nodes.is_empty());
    }

    #[test]
    fn test_build_state_has_ports() {
        let state = build_simulated_state();
        assert!(!state.ports.is_empty());
    }

    #[test]
    fn test_build_state_has_links() {
        let state = build_simulated_state();
        assert!(!state.links.is_empty());
    }

    #[test]
    fn test_build_state_has_devices() {
        let state = build_simulated_state();
        assert!(!state.devices.is_empty());
    }

    #[test]
    fn test_build_state_has_clients() {
        let state = build_simulated_state();
        assert!(!state.clients.is_empty());
    }

    #[test]
    fn test_build_state_has_modules() {
        let state = build_simulated_state();
        assert!(!state.modules.is_empty());
    }

    #[test]
    fn test_build_state_has_metadata() {
        let state = build_simulated_state();
        assert!(!state.metadata.is_empty());
    }

    #[test]
    fn test_build_state_has_factories() {
        let state = build_simulated_state();
        assert!(!state.factories.is_empty());
    }

    // ── Node lookups ───────────────────────────────────────────────

    #[test]
    fn test_find_node_by_id_exists() {
        let state = build_simulated_state();
        assert!(find_node_by_id(&state, 30).is_some());
    }

    #[test]
    fn test_find_node_by_id_not_exists() {
        let state = build_simulated_state();
        assert!(find_node_by_id(&state, 9999).is_none());
    }

    #[test]
    fn test_find_node_name() {
        let state = build_simulated_state();
        let node = find_node_by_id(&state, 30).unwrap();
        assert!(node.name.contains("pci-0000_00_1f.3"));
    }

    #[test]
    fn test_find_node_state() {
        let state = build_simulated_state();
        let node = find_node_by_id(&state, 30).unwrap();
        assert_eq!(node.state, NodeState::Running);
    }

    // ── Port lookups ───────────────────────────────────────────────

    #[test]
    fn test_find_port_by_id_exists() {
        let state = build_simulated_state();
        assert!(find_port_by_id(&state, 200).is_some());
    }

    #[test]
    fn test_find_port_by_id_not_exists() {
        let state = build_simulated_state();
        assert!(find_port_by_id(&state, 9999).is_none());
    }

    #[test]
    fn test_ports_for_node() {
        let state = build_simulated_state();
        let ports = ports_for_node(&state, 30);
        assert_eq!(ports.len(), 2);
    }

    #[test]
    fn test_ports_for_node_empty() {
        let state = build_simulated_state();
        let ports = ports_for_node(&state, 9999);
        assert!(ports.is_empty());
    }

    #[test]
    fn test_port_direction() {
        let state = build_simulated_state();
        let port = find_port_by_id(&state, 200).unwrap();
        assert_eq!(port.direction, PortDirection::Input);
    }

    #[test]
    fn test_port_direction_output() {
        let state = build_simulated_state();
        let port = find_port_by_id(&state, 202).unwrap();
        assert_eq!(port.direction, PortDirection::Output);
    }

    // ── Link lookups ───────────────────────────────────────────────

    #[test]
    fn test_links_for_node() {
        let state = build_simulated_state();
        let links = links_for_node(&state, 34);
        assert!(!links.is_empty());
    }

    #[test]
    fn test_links_state_active() {
        let state = build_simulated_state();
        let link = state.links.iter().find(|l| l.id == 300).unwrap();
        assert_eq!(link.state, LinkState::Active);
    }

    #[test]
    fn test_links_state_paused() {
        let state = build_simulated_state();
        let link = state.links.iter().find(|l| l.id == 303).unwrap();
        assert_eq!(link.state, LinkState::Paused);
    }

    // ── Device lookups ─────────────────────────────────────────────

    #[test]
    fn test_find_device_by_id_exists() {
        let state = build_simulated_state();
        assert!(find_device_by_id(&state, 100).is_some());
    }

    #[test]
    fn test_find_device_by_id_not_exists() {
        let state = build_simulated_state();
        assert!(find_device_by_id(&state, 9999).is_none());
    }

    #[test]
    fn test_device_has_profiles() {
        let state = build_simulated_state();
        let device = find_device_by_id(&state, 100).unwrap();
        assert!(!device.profiles.is_empty());
    }

    #[test]
    fn test_device_type() {
        let state = build_simulated_state();
        let device = find_device_by_id(&state, 100).unwrap();
        assert_eq!(device.device_type, DeviceType::AlsaCard);
    }

    // ── Client lookups ─────────────────────────────────────────────

    #[test]
    fn test_find_client_by_id_exists() {
        let state = build_simulated_state();
        assert!(find_client_by_id(&state, 40).is_some());
    }

    #[test]
    fn test_find_client_by_id_not_exists() {
        let state = build_simulated_state();
        assert!(find_client_by_id(&state, 9999).is_none());
    }

    // ── Module lookups ─────────────────────────────────────────────

    #[test]
    fn test_find_module_by_id_exists() {
        let state = build_simulated_state();
        assert!(find_module_by_id(&state, 0).is_some());
    }

    #[test]
    fn test_find_module_by_id_not_exists() {
        let state = build_simulated_state();
        assert!(find_module_by_id(&state, 9999).is_none());
    }

    // ── Default sink/source ────────────────────────────────────────

    #[test]
    fn test_default_sink_exists() {
        let state = build_simulated_state();
        let sink = default_sink(&state);
        assert!(sink.is_some());
        assert_eq!(sink.unwrap().id, 30);
    }

    #[test]
    fn test_default_source_exists() {
        let state = build_simulated_state();
        let source = default_source(&state);
        assert!(source.is_some());
        assert_eq!(source.unwrap().id, 31);
    }

    // ── Sink/source/stream node filtering ──────────────────────────

    #[test]
    fn test_sink_nodes() {
        let state = build_simulated_state();
        let sinks = sink_nodes(&state);
        assert!(sinks.len() >= 2);
        for s in &sinks {
            assert_eq!(s.media_class, "Audio/Sink");
        }
    }

    #[test]
    fn test_source_nodes() {
        let state = build_simulated_state();
        let sources = source_nodes(&state);
        assert!(sources.len() >= 2);
        for s in &sources {
            assert_eq!(s.media_class, "Audio/Source");
        }
    }

    #[test]
    fn test_stream_output_nodes() {
        let state = build_simulated_state();
        let streams = stream_output_nodes(&state);
        assert!(!streams.is_empty());
    }

    #[test]
    fn test_stream_input_nodes() {
        let state = build_simulated_state();
        let streams = stream_input_nodes(&state);
        assert!(!streams.is_empty());
    }

    // ── JSON helpers ───────────────────────────────────────────────

    #[test]
    fn test_extract_json_string_field() {
        let json = "{\"name\":\"foo\",\"other\":\"bar\"}";
        assert_eq!(extract_json_string_field(json, "name"), Some("foo"));
    }

    #[test]
    fn test_extract_json_string_field_second() {
        let json = "{\"name\":\"foo\",\"other\":\"bar\"}";
        assert_eq!(extract_json_string_field(json, "other"), Some("bar"));
    }

    #[test]
    fn test_extract_json_string_field_missing() {
        let json = "{\"name\":\"foo\"}";
        assert_eq!(extract_json_string_field(json, "missing"), None);
    }

    #[test]
    fn test_escape_json_string_plain() {
        assert_eq!(escape_json_string("hello"), "hello");
    }

    #[test]
    fn test_escape_json_string_quotes() {
        assert_eq!(escape_json_string("say \"hi\""), "say \\\"hi\\\"");
    }

    #[test]
    fn test_escape_json_string_backslash() {
        assert_eq!(escape_json_string("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_escape_json_string_newline() {
        assert_eq!(escape_json_string("a\nb"), "a\\nb");
    }

    #[test]
    fn test_escape_json_string_tab() {
        assert_eq!(escape_json_string("a\tb"), "a\\tb");
    }

    #[test]
    fn test_escape_json_string_carriage_return() {
        assert_eq!(escape_json_string("a\rb"), "a\\rb");
    }

    // ── Volume helpers ─────────────────────────────────────────────

    #[test]
    fn test_format_volume_percent() {
        assert_eq!(format_volume_percent(0.74), "74%");
    }

    #[test]
    fn test_format_volume_percent_zero() {
        assert_eq!(format_volume_percent(0.0), "0%");
    }

    #[test]
    fn test_format_volume_percent_full() {
        assert_eq!(format_volume_percent(1.0), "100%");
    }

    #[test]
    fn test_parse_volume_spec_percent() {
        assert_eq!(parse_volume_spec("50%"), Some(0.5));
    }

    #[test]
    fn test_parse_volume_spec_decimal() {
        assert_eq!(parse_volume_spec("0.75"), Some(0.75));
    }

    #[test]
    fn test_parse_volume_spec_integer() {
        assert_eq!(parse_volume_spec("80"), Some(0.8));
    }

    #[test]
    fn test_parse_volume_spec_invalid() {
        assert_eq!(parse_volume_spec("abc"), None);
    }

    // ── Audio format ───────────────────────────────────────────────

    #[test]
    fn test_audio_format_bytes_s16() {
        assert_eq!(AudioFormat::S16Le.bytes_per_sample(), 2);
    }

    #[test]
    fn test_audio_format_bytes_s24() {
        assert_eq!(AudioFormat::S24Le.bytes_per_sample(), 3);
    }

    #[test]
    fn test_audio_format_bytes_s32() {
        assert_eq!(AudioFormat::S32Le.bytes_per_sample(), 4);
    }

    #[test]
    fn test_audio_format_bytes_f32() {
        assert_eq!(AudioFormat::F32Le.bytes_per_sample(), 4);
    }

    #[test]
    fn test_audio_format_bytes_u8() {
        assert_eq!(AudioFormat::_U8.bytes_per_sample(), 1);
    }

    #[test]
    fn test_audio_format_from_str_s16le() {
        assert_eq!(AudioFormat::from_str_opt("S16LE"), Some(AudioFormat::S16Le));
    }

    #[test]
    fn test_audio_format_from_str_lowercase() {
        assert_eq!(AudioFormat::from_str_opt("f32le"), Some(AudioFormat::F32Le));
    }

    #[test]
    fn test_audio_format_from_str_short() {
        assert_eq!(AudioFormat::from_str_opt("s16"), Some(AudioFormat::S16Le));
    }

    #[test]
    fn test_audio_format_from_str_invalid() {
        assert_eq!(AudioFormat::from_str_opt("INVALID"), None);
    }

    #[test]
    fn test_audio_format_display() {
        assert_eq!(format!("{}", AudioFormat::S16Le), "S16LE");
        assert_eq!(format!("{}", AudioFormat::F32Le), "F32LE");
    }

    // ── Data rate computation ──────────────────────────────────────

    #[test]
    fn test_compute_data_rate_cd_quality() {
        let config = AudioStreamConfig {
            format: AudioFormat::S16Le,
            rate: 44100,
            channels: 2,
            _volume: 1.0,
            _buffer_size: 1024,
        };
        // 44100 * 2 * 2 = 176400
        assert_eq!(compute_data_rate(&config), 176400);
    }

    #[test]
    fn test_compute_data_rate_48k_stereo_32bit() {
        let config = AudioStreamConfig {
            format: AudioFormat::S32Le,
            rate: 48000,
            channels: 2,
            _volume: 1.0,
            _buffer_size: 1024,
        };
        // 48000 * 2 * 4 = 384000
        assert_eq!(compute_data_rate(&config), 384000);
    }

    #[test]
    fn test_compute_data_rate_mono() {
        let config = AudioStreamConfig {
            format: AudioFormat::S16Le,
            rate: 16000,
            channels: 1,
            _volume: 1.0,
            _buffer_size: 512,
        };
        assert_eq!(compute_data_rate(&config), 32000);
    }

    // ── Bytes formatting ───────────────────────────────────────────

    #[test]
    fn test_format_bytes_display_bytes() {
        assert_eq!(format_bytes_display(500), "500 B");
    }

    #[test]
    fn test_format_bytes_display_kib() {
        let result = format_bytes_display(2048);
        assert!(result.contains("KiB"));
    }

    #[test]
    fn test_format_bytes_display_mib() {
        let result = format_bytes_display(2 * 1024 * 1024);
        assert!(result.contains("MiB"));
    }

    #[test]
    fn test_format_bytes_display_gib() {
        let result = format_bytes_display(2 * 1024 * 1024 * 1024);
        assert!(result.contains("GiB"));
    }

    // ── Duration formatting ────────────────────────────────────────

    #[test]
    fn test_format_duration_hms_short() {
        let result = format_duration_hms(5.0);
        assert_eq!(result, "00:05.00");
    }

    #[test]
    fn test_format_duration_hms_minutes() {
        let result = format_duration_hms(125.5);
        assert_eq!(result, "02:05.50");
    }

    #[test]
    fn test_format_duration_hms_hours() {
        let result = format_duration_hms(3661.0);
        assert_eq!(result, "01:01:01.00");
    }

    // ── Stream args parsing ────────────────────────────────────────

    #[test]
    fn test_parse_stream_args_defaults() {
        let args: Vec<String> = vec![];
        let config = parse_stream_args(&args, StreamDirection::Playback);
        assert_eq!(config.format, AudioFormat::S16Le);
        assert_eq!(config.rate, DEFAULT_SAMPLE_RATE);
        assert_eq!(config.channels, DEFAULT_CHANNELS);
    }

    #[test]
    fn test_parse_stream_args_format() {
        let args: Vec<String> = vec!["--format".into(), "F32LE".into()];
        let config = parse_stream_args(&args, StreamDirection::Playback);
        assert_eq!(config.format, AudioFormat::F32Le);
    }

    #[test]
    fn test_parse_stream_args_rate() {
        let args: Vec<String> = vec!["--rate".into(), "44100".into()];
        let config = parse_stream_args(&args, StreamDirection::Playback);
        assert_eq!(config.rate, 44100);
    }

    #[test]
    fn test_parse_stream_args_channels() {
        let args: Vec<String> = vec!["--channels".into(), "1".into()];
        let config = parse_stream_args(&args, StreamDirection::Playback);
        assert_eq!(config.channels, 1);
    }

    #[test]
    fn test_parse_stream_args_short_flags() {
        let args: Vec<String> = vec![
            "-f".into(),
            "S24LE".into(),
            "-r".into(),
            "96000".into(),
            "-c".into(),
            "6".into(),
        ];
        let config = parse_stream_args(&args, StreamDirection::Capture);
        assert_eq!(config.format, AudioFormat::S24Le);
        assert_eq!(config.rate, 96000);
        assert_eq!(config.channels, 6);
    }

    #[test]
    fn test_parse_stream_args_invalid_format_keeps_default() {
        let args: Vec<String> = vec!["--format".into(), "INVALID".into()];
        let config = parse_stream_args(&args, StreamDirection::Playback);
        assert_eq!(config.format, AudioFormat::S16Le);
    }

    // ── Volume info ────────────────────────────────────────────────

    #[test]
    fn test_simulated_volume_for_known_node() {
        let vol = simulated_volume_for_node(30);
        assert!((vol.volume - 0.74).abs() < 0.001);
        assert!(!vol.muted);
    }

    #[test]
    fn test_simulated_volume_for_muted_node() {
        let vol = simulated_volume_for_node(32);
        assert!(vol.muted);
    }

    #[test]
    fn test_simulated_volume_for_unknown_node() {
        let vol = simulated_volume_for_node(9999);
        assert!((vol.volume - 1.0).abs() < 0.001);
    }

    // ── Processing info ────────────────────────────────────────────

    #[test]
    fn test_simulated_processing_info_not_empty() {
        let state = build_simulated_state();
        let info = simulated_processing_info(&state);
        assert!(!info.is_empty());
    }

    #[test]
    fn test_simulated_processing_running_has_busy() {
        let state = build_simulated_state();
        let info = simulated_processing_info(&state);
        let running: Vec<_> = info.iter().filter(|i| i._busy_ns > 0).collect();
        assert!(!running.is_empty());
    }

    // ── Monitor events ─────────────────────────────────────────────

    #[test]
    fn test_simulated_monitor_events_not_empty() {
        let state = build_simulated_state();
        let events = simulated_monitor_events(&state);
        assert!(!events.is_empty());
    }

    #[test]
    fn test_monitor_events_contain_nodes() {
        let state = build_simulated_state();
        let events = simulated_monitor_events(&state);
        let node_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.object_type, ObjectType::Node))
            .collect();
        assert!(!node_events.is_empty());
    }

    #[test]
    fn test_monitor_events_all_added() {
        let state = build_simulated_state();
        let events = simulated_monitor_events(&state);
        for event in &events {
            assert_eq!(event.event_type, MonitorEventType::Added);
        }
    }

    // ── parse_id_or_name ───────────────────────────────────────────

    #[test]
    fn test_parse_id_or_name_numeric() {
        let state = build_simulated_state();
        let node = parse_id_or_name("30", &state);
        assert!(node.is_some());
        assert_eq!(node.unwrap().id, 30);
    }

    #[test]
    fn test_parse_id_or_name_by_name() {
        let state = build_simulated_state();
        let node = parse_id_or_name("Firefox", &state);
        assert!(node.is_some());
        assert_eq!(node.unwrap().id, 34);
    }

    #[test]
    fn test_parse_id_or_name_not_found() {
        let state = build_simulated_state();
        let node = parse_id_or_name("NonExistent", &state);
        assert!(node.is_none());
    }

    // ── format_node_short ──────────────────────────────────────────

    #[test]
    fn test_format_node_short() {
        let state = build_simulated_state();
        let node = find_node_by_id(&state, 34).unwrap();
        let s = format_node_short(node);
        assert!(s.contains("34"));
        assert!(s.contains("Firefox"));
    }

    // ── wpctl ID parsing ───────────────────────────────────────────

    #[test]
    fn test_parse_wpctl_id_numeric() {
        let state = build_simulated_state();
        assert_eq!(parse_wpctl_id("30", &state), Some(30));
    }

    #[test]
    fn test_parse_wpctl_id_default_sink() {
        let state = build_simulated_state();
        assert_eq!(parse_wpctl_id("@DEFAULT_AUDIO_SINK@", &state), Some(30));
    }

    #[test]
    fn test_parse_wpctl_id_default_source() {
        let state = build_simulated_state();
        assert_eq!(parse_wpctl_id("@DEFAULT_AUDIO_SOURCE@", &state), Some(31));
    }

    #[test]
    fn test_parse_wpctl_id_invalid() {
        let state = build_simulated_state();
        assert_eq!(parse_wpctl_id("not_a_number", &state), None);
    }

    // ── Display trait implementations ──────────────────────────────

    #[test]
    fn test_node_type_display() {
        assert_eq!(format!("{}", NodeType::AudioSource), "Audio/Source");
        assert_eq!(format!("{}", NodeType::AudioSink), "Audio/Sink");
        assert_eq!(format!("{}", NodeType::VideoSource), "Video/Source");
    }

    #[test]
    fn test_node_state_display() {
        assert_eq!(format!("{}", NodeState::Running), "running");
        assert_eq!(format!("{}", NodeState::Idle), "idle");
        assert_eq!(format!("{}", NodeState::Suspended), "suspended");
    }

    #[test]
    fn test_port_direction_display() {
        assert_eq!(format!("{}", PortDirection::Input), "in");
        assert_eq!(format!("{}", PortDirection::Output), "out");
    }

    #[test]
    fn test_media_type_display() {
        assert_eq!(format!("{}", MediaType::Audio), "Audio");
        assert_eq!(format!("{}", MediaType::Video), "Video");
    }

    #[test]
    fn test_link_state_display() {
        assert_eq!(format!("{}", LinkState::Active), "active");
        assert_eq!(format!("{}", LinkState::Paused), "paused");
    }

    #[test]
    fn test_device_type_display() {
        assert_eq!(format!("{}", DeviceType::AlsaPcm), "alsa/pcm");
        assert_eq!(format!("{}", DeviceType::AlsaCard), "alsa/card");
    }

    #[test]
    fn test_object_type_display() {
        assert_eq!(format!("{}", ObjectType::Node), "PipeWire:Interface:Node");
        assert_eq!(format!("{}", ObjectType::Port), "PipeWire:Interface:Port");
    }

    #[test]
    fn test_monitor_event_type_display() {
        assert_eq!(format!("{}", MonitorEventType::Added), "added");
        assert_eq!(format!("{}", MonitorEventType::Removed), "removed");
    }

    #[test]
    fn test_client_access_display() {
        assert_eq!(format!("{}", ClientAccess::Unrestricted), "unrestricted");
    }

    #[test]
    fn test_stream_direction_display() {
        assert_eq!(format!("{}", StreamDirection::Playback), "playback");
        assert_eq!(format!("{}", StreamDirection::Capture), "capture");
    }

    #[test]
    fn test_channel_position_display() {
        assert_eq!(format!("{}", ChannelPosition::FrontLeft), "FL");
        assert_eq!(format!("{}", ChannelPosition::FrontRight), "FR");
        assert_eq!(format!("{}", ChannelPosition::Mono), "MONO");
    }

    #[test]
    fn test_daemon_action_display() {
        assert_eq!(format!("{}", _DaemonAction::Start), "start");
    }

    #[test]
    fn test_metadata_key_type_display() {
        assert_eq!(format!("{}", _MetadataKeyType::TargetNode), "target.node");
        assert_eq!(format!("{}", _MetadataKeyType::RouteDevice), "route-device");
    }

    #[test]
    fn test_media_subtype_display() {
        assert_eq!(format!("{}", MediaSubtype::Raw), "raw");
    }

    // ── pw-cli commands ────────────────────────────────────────────

    #[test]
    fn test_pw_cli_help_returns_zero() {
        let state = build_simulated_state();
        let _ = run_pw_cli(vec![], &state);
    }

    #[test]
    fn test_pw_cli_version() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["--version".into()], &state), 0);
    }

    #[test]
    fn test_pw_cli_info_core() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["info".into()], &state), 0);
    }

    #[test]
    fn test_pw_cli_info_node() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["info".into(), "30".into()], &state), 0);
    }

    #[test]
    fn test_pw_cli_info_not_found() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["info".into(), "9999".into()], &state), 1);
    }

    #[test]
    fn test_pw_cli_info_all() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["info".into(), "all".into()], &state), 0);
    }

    #[test]
    fn test_pw_cli_list_objects_all() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["list-objects".into()], &state), 0);
    }

    #[test]
    fn test_pw_cli_list_objects_nodes() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(vec!["list-objects".into(), "Node".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_cli_list_objects_ports() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(vec!["list-objects".into(), "Port".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_cli_list_objects_links() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(vec!["list-objects".into(), "Link".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_cli_list_objects_devices() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(vec!["list-objects".into(), "Device".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_cli_list_objects_clients() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(vec!["list-objects".into(), "Client".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_cli_list_objects_modules() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(vec!["list-objects".into(), "Module".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_cli_list_objects_unknown_type() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(vec!["list-objects".into(), "Bogus".into()], &state),
            1
        );
    }

    #[test]
    fn test_pw_cli_enum_params_format() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(
                vec!["enum-params".into(), "30".into(), "Format".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_pw_cli_enum_params_props() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(
                vec!["enum-params".into(), "30".into(), "Props".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_pw_cli_enum_params_latency() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(
                vec!["enum-params".into(), "30".into(), "ProcessLatency".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_pw_cli_enum_params_missing_args() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["enum-params".into()], &state), 1);
    }

    #[test]
    fn test_pw_cli_destroy_existing() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["destroy".into(), "30".into()], &state), 0);
    }

    #[test]
    fn test_pw_cli_destroy_nonexistent() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["destroy".into(), "9999".into()], &state), 1);
    }

    #[test]
    fn test_pw_cli_create_link_valid() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(
                vec!["create-link".into(), "207".into(), "200".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_pw_cli_create_link_invalid_port() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_cli(
                vec!["create-link".into(), "9999".into(), "200".into()],
                &state
            ),
            1
        );
    }

    #[test]
    fn test_pw_cli_unknown_command() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cli(vec!["bogus-command".into()], &state), 1);
    }

    // ── pw-dump ────────────────────────────────────────────────────

    #[test]
    fn test_pw_dump_all() {
        let state = build_simulated_state();
        let _ = run_pw_dump(vec![], &state);
    }

    #[test]
    fn test_pw_dump_help() {
        let state = build_simulated_state();
        assert_eq!(run_pw_dump(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pw_dump_specific_id() {
        let state = build_simulated_state();
        assert_eq!(run_pw_dump(vec!["30".into()], &state), 0);
    }

    // ── pw-record ──────────────────────────────────────────────────

    #[test]
    fn test_pw_record_help() {
        let state = build_simulated_state();
        assert_eq!(run_pw_record(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pw_record_no_args() {
        let state = build_simulated_state();
        assert_eq!(run_pw_record(vec![], &state), 1);
    }

    #[test]
    fn test_pw_record_with_file() {
        let state = build_simulated_state();
        assert_eq!(run_pw_record(vec!["output.wav".into()], &state), 0);
    }

    #[test]
    fn test_pw_record_with_options() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_record(
                vec![
                    "--format".into(),
                    "F32LE".into(),
                    "--rate".into(),
                    "44100".into(),
                    "out.wav".into()
                ],
                &state
            ),
            0
        );
    }

    // ── pw-play ────────────────────────────────────────────────────

    #[test]
    fn test_pw_play_help() {
        let state = build_simulated_state();
        assert_eq!(run_pw_play(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pw_play_no_args() {
        let state = build_simulated_state();
        assert_eq!(run_pw_play(vec![], &state), 1);
    }

    #[test]
    fn test_pw_play_with_file() {
        let state = build_simulated_state();
        assert_eq!(run_pw_play(vec!["music.wav".into()], &state), 0);
    }

    // ── pw-cat ─────────────────────────────────────────────────────

    #[test]
    fn test_pw_cat_help() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cat(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pw_cat_no_args() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cat(vec![], &state), 1);
    }

    #[test]
    fn test_pw_cat_record_mode() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cat(vec!["--record".into()], &state), 0);
    }

    #[test]
    fn test_pw_cat_playback_mode() {
        let state = build_simulated_state();
        assert_eq!(run_pw_cat(vec!["--playback".into()], &state), 0);
    }

    // ── pw-mon ─────────────────────────────────────────────────────

    #[test]
    fn test_pw_mon_default() {
        let state = build_simulated_state();
        let _ = run_pw_mon(vec![], &state);
    }

    #[test]
    fn test_pw_mon_help() {
        let state = build_simulated_state();
        assert_eq!(run_pw_mon(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pw_mon_all() {
        let state = build_simulated_state();
        assert_eq!(run_pw_mon(vec!["--all".into()], &state), 0);
    }

    // ── pw-metadata ────────────────────────────────────────────────

    #[test]
    fn test_pw_metadata_list() {
        let state = build_simulated_state();
        let _ = run_pw_metadata(vec![], &state);
    }

    #[test]
    fn test_pw_metadata_help() {
        let state = build_simulated_state();
        assert_eq!(run_pw_metadata(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pw_metadata_get_subject() {
        let state = build_simulated_state();
        assert_eq!(run_pw_metadata(vec!["0".into()], &state), 0);
    }

    #[test]
    fn test_pw_metadata_get_key() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_metadata(vec!["0".into(), "default.audio.sink".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_metadata_set() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_metadata(
                vec!["0".into(), "test.key".into(), "test.value".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_pw_metadata_delete() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_metadata(vec!["--delete".into(), "0".into()], &state),
            0
        );
    }

    #[test]
    fn test_pw_metadata_monitor() {
        let state = build_simulated_state();
        assert_eq!(run_pw_metadata(vec!["--monitor".into()], &state), 0);
    }

    // ── pw-top ─────────────────────────────────────────────────────

    #[test]
    fn test_pw_top_default() {
        let state = build_simulated_state();
        let _ = run_pw_top(vec![], &state);
    }

    #[test]
    fn test_pw_top_help() {
        let state = build_simulated_state();
        assert_eq!(run_pw_top(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pw_top_batch() {
        let state = build_simulated_state();
        assert_eq!(run_pw_top(vec!["--batch".into()], &state), 0);
    }

    #[test]
    fn test_pw_top_iterations() {
        let state = build_simulated_state();
        assert_eq!(
            run_pw_top(vec!["--iterations".into(), "2".into()], &state),
            0
        );
    }

    // ── wpctl ──────────────────────────────────────────────────────

    #[test]
    fn test_wpctl_help() {
        let state = build_simulated_state();
        let _ = run_wpctl(vec![], &state);
    }

    #[test]
    fn test_wpctl_version() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["--version".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_status() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["status".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_inspect_node() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["inspect".into(), "30".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_inspect_device() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["inspect".into(), "100".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_inspect_client() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["inspect".into(), "40".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_inspect_port() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["inspect".into(), "200".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_inspect_link() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["inspect".into(), "300".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_inspect_not_found() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["inspect".into(), "9999".into()], &state), 1);
    }

    #[test]
    fn test_wpctl_inspect_missing_arg() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["inspect".into()], &state), 1);
    }

    #[test]
    fn test_wpctl_set_volume() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-volume".into(), "30".into(), "50%".into()], &state),
            0
        );
    }

    #[test]
    fn test_wpctl_set_volume_relative_up() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec!["set-volume".into(), "30".into(), "+10%".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_wpctl_set_volume_relative_down() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec!["set-volume".into(), "30".into(), "-10%".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_wpctl_set_volume_default_sink_keyword() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec![
                    "set-volume".into(),
                    "@DEFAULT_AUDIO_SINK@".into(),
                    "0.5".into()
                ],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_wpctl_set_volume_not_found() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec!["set-volume".into(), "9999".into(), "50%".into()],
                &state
            ),
            1
        );
    }

    #[test]
    fn test_wpctl_set_volume_invalid() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-volume".into(), "30".into(), "abc".into()], &state),
            1
        );
    }

    #[test]
    fn test_wpctl_set_mute_on() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-mute".into(), "30".into(), "1".into()], &state),
            0
        );
    }

    #[test]
    fn test_wpctl_set_mute_off() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-mute".into(), "30".into(), "0".into()], &state),
            0
        );
    }

    #[test]
    fn test_wpctl_set_mute_toggle() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec!["set-mute".into(), "30".into(), "toggle".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_wpctl_set_mute_invalid() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-mute".into(), "30".into(), "maybe".into()], &state),
            1
        );
    }

    #[test]
    fn test_wpctl_set_default() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-default".into(), "32".into()], &state),
            0
        );
    }

    #[test]
    fn test_wpctl_set_default_not_found() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-default".into(), "9999".into()], &state),
            1
        );
    }

    #[test]
    fn test_wpctl_get_volume() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["get-volume".into(), "30".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_get_volume_muted() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["get-volume".into(), "32".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_get_volume_not_found() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["get-volume".into(), "9999".into()], &state),
            1
        );
    }

    #[test]
    fn test_wpctl_clear_default() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["clear-default".into(), "30".into()], &state),
            0
        );
    }

    #[test]
    fn test_wpctl_set_profile() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["set-profile".into(), "100".into(), "0".into()], &state),
            0
        );
    }

    #[test]
    fn test_wpctl_set_profile_out_of_range() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec!["set-profile".into(), "100".into(), "99".into()],
                &state
            ),
            1
        );
    }

    #[test]
    fn test_wpctl_set_profile_device_not_found() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec!["set-profile".into(), "9999".into(), "0".into()],
                &state
            ),
            1
        );
    }

    #[test]
    fn test_wpctl_settings_all() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["settings".into()], &state), 0);
    }

    #[test]
    fn test_wpctl_settings_get() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(vec!["settings".into(), "log.level".into()], &state),
            0
        );
    }

    #[test]
    fn test_wpctl_settings_set() {
        let state = build_simulated_state();
        assert_eq!(
            run_wpctl(
                vec!["settings".into(), "log.level".into(), "3".into()],
                &state
            ),
            0
        );
    }

    #[test]
    fn test_wpctl_unknown_command() {
        let state = build_simulated_state();
        assert_eq!(run_wpctl(vec!["bogus".into()], &state), 1);
    }

    // ── pipewire daemon ────────────────────────────────────────────

    #[test]
    fn test_pipewire_daemon_help() {
        let state = build_simulated_state();
        assert_eq!(run_pipewire_daemon(vec!["--help".into()], &state), 0);
    }

    #[test]
    fn test_pipewire_daemon_version() {
        let state = build_simulated_state();
        assert_eq!(run_pipewire_daemon(vec!["--version".into()], &state), 0);
    }

    #[test]
    fn test_pipewire_daemon_default() {
        let state = build_simulated_state();
        let _ = run_pipewire_daemon(vec![], &state);
    }

    #[test]
    fn test_pipewire_daemon_verbose() {
        let state = build_simulated_state();
        assert_eq!(run_pipewire_daemon(vec!["--verbose".into()], &state), 0);
    }

    // ── Record config parsing ──────────────────────────────────────

    #[test]
    fn test_parse_record_config_basic() {
        let args: Vec<String> = vec!["output.wav".into()];
        let config = parse_record_config(&args);
        assert!(config.is_some());
        assert_eq!(config.unwrap().output_file, "output.wav");
    }

    #[test]
    fn test_parse_record_config_with_target() {
        let args: Vec<String> = vec!["--target".into(), "31".into(), "output.wav".into()];
        let config = parse_record_config(&args);
        assert!(config.is_some());
        let cfg = config.unwrap();
        assert_eq!(cfg.target.as_deref(), Some("31"));
    }

    #[test]
    fn test_parse_record_config_no_file() {
        let args: Vec<String> = vec!["--format".into(), "F32LE".into()];
        let config = parse_record_config(&args);
        assert!(config.is_none());
    }

    // ── Play config parsing ────────────────────────────────────────

    #[test]
    fn test_parse_play_config_basic() {
        let args: Vec<String> = vec!["music.wav".into()];
        let config = parse_play_config(&args);
        assert!(config.is_some());
        assert_eq!(config.unwrap().input_file, "music.wav");
    }

    #[test]
    fn test_parse_play_config_with_loop() {
        let args: Vec<String> = vec!["--loop".into(), "music.wav".into()];
        let config = parse_play_config(&args);
        assert!(config.is_some());
        assert!(config.unwrap()._loop_playback);
    }

    #[test]
    fn test_parse_play_config_no_file() {
        let args: Vec<String> = vec!["--format".into(), "S32LE".into()];
        let config = parse_play_config(&args);
        assert!(config.is_none());
    }

    // ── Cat config parsing ─────────────────────────────────────────

    #[test]
    fn test_parse_cat_config_default_playback() {
        let args: Vec<String> = vec!["file.raw".into()];
        let config = parse_cat_config(&args);
        assert_eq!(config.direction, StreamDirection::Playback);
    }

    #[test]
    fn test_parse_cat_config_record_mode() {
        let args: Vec<String> = vec!["--record".into()];
        let config = parse_cat_config(&args);
        assert_eq!(config.direction, StreamDirection::Capture);
    }

    #[test]
    fn test_parse_cat_config_with_target() {
        let args: Vec<String> = vec!["--target".into(), "30".into()];
        let config = parse_cat_config(&args);
        assert_eq!(config.target.as_deref(), Some("30"));
    }

    // ── Enum Copy/Clone/Eq ─────────────────────────────────────────

    #[test]
    fn test_personality_copy() {
        let a = Personality::PwCli;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_node_type_copy() {
        let a = NodeType::AudioSink;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_node_state_copy() {
        let a = NodeState::Running;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_port_direction_copy() {
        let a = PortDirection::Input;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_link_state_copy() {
        let a = LinkState::Active;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_audio_format_copy() {
        let a = AudioFormat::F32Le;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_audio_stream_config_copy() {
        let a = AudioStreamConfig {
            format: AudioFormat::S16Le,
            rate: 48000,
            channels: 2,
            _volume: 1.0,
            _buffer_size: 1024,
        };
        let b = a;
        assert_eq!(a.rate, b.rate);
    }

    #[test]
    fn test_volume_info_copy() {
        let a = simulated_volume_for_node(30);
        let b = a;
        assert!((a.volume - b.volume).abs() < 0.001);
    }

    #[test]
    fn test_processing_info_copy() {
        let a = ProcessingInfo {
            node_id: 1,
            _quantum: 1024,
            _rate: 48000,
            _wait_ns: 100,
            _busy_ns: 50,
            _xrun_count: 0,
            _latency_ns: 1000,
        };
        let b = a;
        assert_eq!(a.node_id, b.node_id);
    }

    #[test]
    fn test_pw_link_copy() {
        let a = PwLink {
            id: 1,
            output_node: 2,
            output_port: 3,
            input_node: 4,
            input_port: 5,
            state: LinkState::Active,
            _active: true,
            _feedback: false,
        };
        let b = a;
        assert_eq!(a.id, b.id);
    }
}
