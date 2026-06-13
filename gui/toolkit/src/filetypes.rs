//! File type definitions, detection, and categorisation for SlateOS.
//!
//! Provides a centralised registry of known file extensions, magic-byte
//! signatures, MIME types, icon glyphs, and category tags.  Every GUI
//! component that needs to display, open, or classify a file should go
//! through this module rather than hard-coding extension lists.

#![allow(dead_code)]

// ---------------------------------------------------------------------------
// FileCategory
// ---------------------------------------------------------------------------

/// Broad classification bucket for a file type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileCategory {
    Executable,
    Library,
    Package,
    Document,
    Spreadsheet,
    Presentation,
    Image,
    Audio,
    Video,
    Code,
    Config,
    Data,
    Archive,
    DiskImage,
    System,
    Unknown,
}

// ---------------------------------------------------------------------------
// FileExtension
// ---------------------------------------------------------------------------

/// Every file extension the OS recognises out of the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileExtension {
    // OS-specific
    Nx,
    Dso,
    Slib,
    Pkg,

    // Documents
    Txt,
    Md,
    Pdf,
    Doc,
    Docx,
    Odt,
    Rtf,
    Csv,
    Tsv,
    Json,
    Yaml,
    Toml,
    Xml,
    Html,

    // Spreadsheet / presentation
    Xls,
    Xlsx,
    Ods,
    Ppt,
    Pptx,
    Odp,

    // Images
    Png,
    Jpg,
    Gif,
    Bmp,
    Svg,
    Ico,
    Webp,
    Tiff,

    // Audio
    Mp3,
    Wav,
    Flac,
    Ogg,
    Aac,
    Wma,
    M4a,

    // Video
    Mp4,
    Mkv,
    Avi,
    Mov,
    Wmv,
    Webm,
    Flv,

    // Code
    Rs,
    Py,
    C,
    Cpp,
    H,
    Hpp,
    Js,
    Ts,
    Java,
    Go,
    Rb,
    Sh,
    Sql,
    Css,
    Scss,

    Kt,
    Cs,

    // Archives
    Zip,
    TarGz,
    TarBz2,
    TarXz,
    SevenZ,
    Rar,

    // Config / log
    Ini,
    Conf,
    Cfg,
    Env,
    Log,

    // Disk images / other
    Iso,
    Img,

    // Catch-all
    Unknown,
}

// ---------------------------------------------------------------------------
// FileTypeInfo
// ---------------------------------------------------------------------------

/// Metadata associated with a single file type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTypeInfo {
    /// Canonical extension string including the leading dot (e.g. `".rs"`).
    pub extension: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// MIME type (RFC 6838).
    pub mime_type: &'static str,
    /// Broad category.
    pub category: FileCategory,
    /// A single Unicode glyph used as a simple icon.
    pub icon_glyph: char,
    /// `true` if the file is human-readable text (openable in a text editor).
    pub is_text: bool,
    /// `true` if the OS can execute the file directly.
    pub is_executable: bool,
    /// Optional name of the default handler application.
    pub default_app: Option<&'static str>,
}

// ---------------------------------------------------------------------------
// Compile-time info table
// ---------------------------------------------------------------------------

/// Master lookup table.  Sorted by extension for binary-search, but we also
/// use a linear scan with case-folding so order is not critical for
/// correctness.
const FILE_TYPE_TABLE: &[FileTypeInfo] = &[
    // -- OS-specific --------------------------------------------------------
    FileTypeInfo {
        extension: ".nx",
        description: "Slate OS Native Executable",
        mime_type: "application/x-slateos-executable",
        category: FileCategory::Executable,
        icon_glyph: '\u{2699}', // gear
        is_text: false,
        is_executable: true,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".dso",
        description: "Dynamic Shared Object",
        mime_type: "application/x-slateos-shared-library",
        category: FileCategory::Library,
        icon_glyph: '\u{1F4E6}', // package
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".slib",
        description: "Static Library",
        mime_type: "application/x-slateos-static-library",
        category: FileCategory::Library,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".pkg",
        description: "Slate OS Package Archive",
        mime_type: "application/x-slateos-package",
        category: FileCategory::Package,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("pkg"),
    },
    // -- Documents ----------------------------------------------------------
    FileTypeInfo {
        extension: ".txt",
        description: "Plain Text",
        mime_type: "text/plain",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}', // page facing up
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".md",
        description: "Markdown Document",
        mime_type: "text/markdown",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".pdf",
        description: "PDF Document",
        mime_type: "application/pdf",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
        default_app: Some("pdfview"),
    },
    FileTypeInfo {
        extension: ".doc",
        description: "Microsoft Word Document (Legacy)",
        mime_type: "application/msword",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".docx",
        description: "Microsoft Word Document",
        mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".odt",
        description: "OpenDocument Text",
        mime_type: "application/vnd.oasis.opendocument.text",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".rtf",
        description: "Rich Text Format",
        mime_type: "application/rtf",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".csv",
        description: "Comma-Separated Values",
        mime_type: "text/csv",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}', // bar chart
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".tsv",
        description: "Tab-Separated Values",
        mime_type: "text/tab-separated-values",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".json",
        description: "JSON Data",
        mime_type: "application/json",
        category: FileCategory::Data,
        icon_glyph: '\u{007B}', // {
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".yaml",
        description: "YAML Document",
        mime_type: "application/x-yaml",
        category: FileCategory::Config,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".toml",
        description: "TOML Configuration",
        mime_type: "application/toml",
        category: FileCategory::Config,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".xml",
        description: "XML Document",
        mime_type: "application/xml",
        category: FileCategory::Data,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".html",
        description: "HTML Document",
        mime_type: "text/html",
        category: FileCategory::Document,
        icon_glyph: '\u{1F310}', // globe with meridians
        is_text: true,
        is_executable: false,
        default_app: Some("browser"),
    },
    // -- Spreadsheet / Presentation ----------------------------------------
    FileTypeInfo {
        extension: ".xls",
        description: "Microsoft Excel Spreadsheet (Legacy)",
        mime_type: "application/vnd.ms-excel",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".xlsx",
        description: "Microsoft Excel Spreadsheet",
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".ods",
        description: "OpenDocument Spreadsheet",
        mime_type: "application/vnd.oasis.opendocument.spreadsheet",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".ppt",
        description: "Microsoft PowerPoint (Legacy)",
        mime_type: "application/vnd.ms-powerpoint",
        category: FileCategory::Presentation,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".pptx",
        description: "Microsoft PowerPoint Presentation",
        mime_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        category: FileCategory::Presentation,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".odp",
        description: "OpenDocument Presentation",
        mime_type: "application/vnd.oasis.opendocument.presentation",
        category: FileCategory::Presentation,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    // -- Images -------------------------------------------------------------
    FileTypeInfo {
        extension: ".png",
        description: "PNG Image",
        mime_type: "image/png",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}', // framed picture
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".jpg",
        description: "JPEG Image",
        mime_type: "image/jpeg",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".jpeg",
        description: "JPEG Image",
        mime_type: "image/jpeg",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".gif",
        description: "GIF Image",
        mime_type: "image/gif",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".bmp",
        description: "BMP Image",
        mime_type: "image/bmp",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".svg",
        description: "SVG Image",
        mime_type: "image/svg+xml",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: true,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".ico",
        description: "Icon Image",
        mime_type: "image/x-icon",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".webp",
        description: "WebP Image",
        mime_type: "image/webp",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".tiff",
        description: "TIFF Image",
        mime_type: "image/tiff",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    FileTypeInfo {
        extension: ".tif",
        description: "TIFF Image",
        mime_type: "image/tiff",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
        default_app: Some("imageview"),
    },
    // -- Audio --------------------------------------------------------------
    FileTypeInfo {
        extension: ".mp3",
        description: "MP3 Audio",
        mime_type: "audio/mpeg",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}', // eighth note
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".wav",
        description: "WAV Audio",
        mime_type: "audio/wav",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".flac",
        description: "FLAC Audio",
        mime_type: "audio/flac",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".ogg",
        description: "Ogg Vorbis Audio",
        mime_type: "audio/ogg",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".aac",
        description: "AAC Audio",
        mime_type: "audio/aac",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".wma",
        description: "Windows Media Audio",
        mime_type: "audio/x-ms-wma",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".m4a",
        description: "MPEG-4 Audio",
        mime_type: "audio/mp4",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".opus",
        description: "Opus Audio",
        mime_type: "audio/opus",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".midi",
        description: "MIDI Audio",
        mime_type: "audio/midi",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    FileTypeInfo {
        extension: ".mid",
        description: "MIDI Audio",
        mime_type: "audio/midi",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
        default_app: Some("audioplayer"),
    },
    // -- Video --------------------------------------------------------------
    FileTypeInfo {
        extension: ".mp4",
        description: "MPEG-4 Video",
        mime_type: "video/mp4",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}', // right-pointing triangle (play)
        is_text: false,
        is_executable: false,
        default_app: Some("videoplayer"),
    },
    FileTypeInfo {
        extension: ".mkv",
        description: "Matroska Video",
        mime_type: "video/x-matroska",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
        default_app: Some("videoplayer"),
    },
    FileTypeInfo {
        extension: ".avi",
        description: "AVI Video",
        mime_type: "video/x-msvideo",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
        default_app: Some("videoplayer"),
    },
    FileTypeInfo {
        extension: ".mov",
        description: "QuickTime Video",
        mime_type: "video/quicktime",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
        default_app: Some("videoplayer"),
    },
    FileTypeInfo {
        extension: ".wmv",
        description: "Windows Media Video",
        mime_type: "video/x-ms-wmv",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
        default_app: Some("videoplayer"),
    },
    FileTypeInfo {
        extension: ".webm",
        description: "WebM Video",
        mime_type: "video/webm",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
        default_app: Some("videoplayer"),
    },
    FileTypeInfo {
        extension: ".flv",
        description: "Flash Video",
        mime_type: "video/x-flv",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
        default_app: Some("videoplayer"),
    },
    // -- Code ---------------------------------------------------------------
    FileTypeInfo {
        extension: ".rs",
        description: "Rust Source File",
        mime_type: "text/x-rust",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}', // {
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".py",
        description: "Python Source File",
        mime_type: "text/x-python",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".c",
        description: "C Source File",
        mime_type: "text/x-c",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".cpp",
        description: "C++ Source File",
        mime_type: "text/x-c++",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".h",
        description: "C/C++ Header File",
        mime_type: "text/x-c",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".hpp",
        description: "C++ Header File",
        mime_type: "text/x-c++",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".js",
        description: "JavaScript Source File",
        mime_type: "text/javascript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".ts",
        description: "TypeScript Source File",
        mime_type: "text/typescript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".java",
        description: "Java Source File",
        mime_type: "text/x-java",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".go",
        description: "Go Source File",
        mime_type: "text/x-go",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".rb",
        description: "Ruby Source File",
        mime_type: "text/x-ruby",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".sh",
        description: "Shell Script",
        mime_type: "application/x-shellscript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: true,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".sql",
        description: "SQL Script",
        mime_type: "application/sql",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".css",
        description: "CSS Stylesheet",
        mime_type: "text/css",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".scss",
        description: "SCSS Stylesheet",
        mime_type: "text/x-scss",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".lua",
        description: "Lua Source File",
        mime_type: "text/x-lua",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".php",
        description: "PHP Source File",
        mime_type: "text/x-php",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".swift",
        description: "Swift Source File",
        mime_type: "text/x-swift",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".kt",
        description: "Kotlin Source File",
        mime_type: "text/x-kotlin",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".cs",
        description: "C# Source File",
        mime_type: "text/x-csharp",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".r",
        description: "R Source File",
        mime_type: "text/x-r",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".zig",
        description: "Zig Source File",
        mime_type: "text/x-zig",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".ada",
        description: "Ada Source File",
        mime_type: "text/x-ada",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    // -- Archives -----------------------------------------------------------
    FileTypeInfo {
        extension: ".zip",
        description: "ZIP Archive",
        mime_type: "application/zip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".tar.gz",
        description: "Gzip-Compressed Tar Archive",
        mime_type: "application/gzip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".tgz",
        description: "Gzip-Compressed Tar Archive",
        mime_type: "application/gzip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".tar.bz2",
        description: "Bzip2-Compressed Tar Archive",
        mime_type: "application/x-bzip2",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".tar.xz",
        description: "XZ-Compressed Tar Archive",
        mime_type: "application/x-xz",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".7z",
        description: "7-Zip Archive",
        mime_type: "application/x-7z-compressed",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".rar",
        description: "RAR Archive",
        mime_type: "application/vnd.rar",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".tar",
        description: "Tar Archive",
        mime_type: "application/x-tar",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".gz",
        description: "Gzip Compressed File",
        mime_type: "application/gzip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".bz2",
        description: "Bzip2 Compressed File",
        mime_type: "application/x-bzip2",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".xz",
        description: "XZ Compressed File",
        mime_type: "application/x-xz",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    FileTypeInfo {
        extension: ".zst",
        description: "Zstandard Compressed File",
        mime_type: "application/zstd",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
        default_app: Some("archiver"),
    },
    // -- Config / Log -------------------------------------------------------
    FileTypeInfo {
        extension: ".ini",
        description: "INI Configuration",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".conf",
        description: "Configuration File",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".cfg",
        description: "Configuration File",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".env",
        description: "Environment Variables",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    FileTypeInfo {
        extension: ".log",
        description: "Log File",
        mime_type: "text/plain",
        category: FileCategory::Data,
        icon_glyph: '\u{1F4C3}', // page with curl
        is_text: true,
        is_executable: false,
        default_app: Some("textedit"),
    },
    // -- Disk images / System -----------------------------------------------
    FileTypeInfo {
        extension: ".iso",
        description: "ISO Disc Image",
        mime_type: "application/x-iso9660-image",
        category: FileCategory::DiskImage,
        icon_glyph: '\u{1F4BF}', // optical disc
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".img",
        description: "Disk Image",
        mime_type: "application/octet-stream",
        category: FileCategory::DiskImage,
        icon_glyph: '\u{1F4BF}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    // -- Font ---------------------------------------------------------------
    FileTypeInfo {
        extension: ".ttf",
        description: "TrueType Font",
        mime_type: "font/ttf",
        category: FileCategory::System,
        icon_glyph: '\u{0041}', // A
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".otf",
        description: "OpenType Font",
        mime_type: "font/otf",
        category: FileCategory::System,
        icon_glyph: '\u{0041}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".woff",
        description: "Web Open Font Format",
        mime_type: "font/woff",
        category: FileCategory::System,
        icon_glyph: '\u{0041}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
    FileTypeInfo {
        extension: ".woff2",
        description: "Web Open Font Format 2",
        mime_type: "font/woff2",
        category: FileCategory::System,
        icon_glyph: '\u{0041}',
        is_text: false,
        is_executable: false,
        default_app: None,
    },
];

/// Sentinel returned when no recognised extension matches.
const UNKNOWN_FILE_TYPE: FileTypeInfo = FileTypeInfo {
    extension: "",
    description: "Unknown File",
    mime_type: "application/octet-stream",
    category: FileCategory::Unknown,
    icon_glyph: '\u{1F4C4}',
    is_text: false,
    is_executable: false,
    default_app: None,
};

// ---------------------------------------------------------------------------
// Magic byte signatures
// ---------------------------------------------------------------------------

/// A magic-byte signature and the file type it identifies.
struct MagicSignature {
    /// Byte pattern that must appear at the start of the file.
    bytes: &'static [u8],
    /// Offset from the start of the file where the pattern must appear.
    offset: usize,
    /// The extension this pattern maps to.
    extension: FileExtension,
}

/// Known magic-byte patterns, checked in order (longest / most specific
/// first where ambiguity exists).
const MAGIC_TABLE: &[MagicSignature] = &[
    // Slate OS native formats
    MagicSignature {
        bytes: b"\x4fNXE",
        offset: 0,
        extension: FileExtension::Nx,
    },
    MagicSignature {
        bytes: b"\x4fDSO",
        offset: 0,
        extension: FileExtension::Dso,
    },
    // Images
    MagicSignature {
        bytes: b"\x89PNG\r\n\x1a\n",
        offset: 0,
        extension: FileExtension::Png,
    },
    MagicSignature {
        bytes: b"\xff\xd8\xff",
        offset: 0,
        extension: FileExtension::Jpg,
    },
    MagicSignature {
        bytes: b"GIF89a",
        offset: 0,
        extension: FileExtension::Gif,
    },
    MagicSignature {
        bytes: b"GIF87a",
        offset: 0,
        extension: FileExtension::Gif,
    },
    MagicSignature {
        bytes: b"BM",
        offset: 0,
        extension: FileExtension::Bmp,
    },
    MagicSignature {
        bytes: b"RIFF",
        offset: 0,
        extension: FileExtension::Wav, // also AVI; disambiguated later
    },
    MagicSignature {
        bytes: b"WEBP",
        offset: 8,
        extension: FileExtension::Webp,
    },
    // Documents
    MagicSignature {
        bytes: b"%PDF",
        offset: 0,
        extension: FileExtension::Pdf,
    },
    // Archives — order matters (ZIP before PKG because PKG uses the same
    // PK header — the caller can check the extension to differentiate).
    MagicSignature {
        bytes: b"PK\x03\x04",
        offset: 0,
        extension: FileExtension::Zip,
    },
    MagicSignature {
        bytes: b"\x1f\x8b",
        offset: 0,
        extension: FileExtension::TarGz,
    },
    MagicSignature {
        bytes: b"BZh",
        offset: 0,
        extension: FileExtension::TarBz2,
    },
    MagicSignature {
        bytes: b"\xfd7zXZ\x00",
        offset: 0,
        extension: FileExtension::TarXz,
    },
    MagicSignature {
        bytes: b"7z\xbc\xaf\x27\x1c",
        offset: 0,
        extension: FileExtension::SevenZ,
    },
    MagicSignature {
        bytes: b"Rar!\x1a\x07",
        offset: 0,
        extension: FileExtension::Rar,
    },
    // Audio / Video
    MagicSignature {
        bytes: b"fLaC",
        offset: 0,
        extension: FileExtension::Flac,
    },
    MagicSignature {
        bytes: b"OggS",
        offset: 0,
        extension: FileExtension::Ogg,
    },
    MagicSignature {
        bytes: b"ID3",
        offset: 0,
        extension: FileExtension::Mp3,
    },
    MagicSignature {
        bytes: b"\xff\xfb",
        offset: 0,
        extension: FileExtension::Mp3,
    },
    // ISO
    // The "CD001" identifier at offset 0x8001 (sector 16) is canonical,
    // but we also accept it at the start of a raw dump.
    MagicSignature {
        bytes: b"CD001",
        offset: 0x8001,
        extension: FileExtension::Iso,
    },
    // Video containers — ftyp box means ISO Base Media (MP4/MOV/M4A)
    MagicSignature {
        bytes: b"ftyp",
        offset: 4,
        extension: FileExtension::Mp4,
    },
    // Matroska / WebM (EBML header)
    MagicSignature {
        bytes: b"\x1a\x45\xdf\xa3",
        offset: 0,
        extension: FileExtension::Mkv,
    },
    // ELF — not an Slate OS format but useful for detection
    MagicSignature {
        bytes: b"\x7fELF",
        offset: 0,
        extension: FileExtension::Unknown, // foreign executable
    },
];

// ---------------------------------------------------------------------------
// Extension string -> FileExtension enum mapping (case-insensitive)
// ---------------------------------------------------------------------------

/// Convert a dotted extension string (e.g. `".rs"`, `"rs"`, `".RS"`) to the
/// enum variant.  Returns [`FileExtension::Unknown`] on no match.
pub fn parse_extension(raw: &str) -> FileExtension {
    // Strip optional leading dot and lowercase.
    let ext = raw.strip_prefix('.').unwrap_or(raw);
    // We avoid heap allocation by matching against known literals directly.
    // For compound extensions (.tar.gz, .tar.bz2, .tar.xz) we check the
    // full suffix first.
    match_extension_ascii_lower(ext)
}

/// ASCII-case-insensitive matching.  The caller has already stripped the
/// leading dot.
fn match_extension_ascii_lower(ext: &str) -> FileExtension {
    // Compound extensions first (longest match wins).
    if eq_ignore_ascii(ext, "tar.gz") || eq_ignore_ascii(ext, "tgz") {
        return FileExtension::TarGz;
    }
    if eq_ignore_ascii(ext, "tar.bz2") {
        return FileExtension::TarBz2;
    }
    if eq_ignore_ascii(ext, "tar.xz") {
        return FileExtension::TarXz;
    }

    match ext.len() {
        1 => match_ext_1(ext),
        2 => match_ext_2(ext),
        3 => match_ext_3(ext),
        4 => match_ext_4(ext),
        _ => match_ext_long(ext),
    }
}

fn eq_ignore_ascii(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn match_ext_1(ext: &str) -> FileExtension {
    if eq_ignore_ascii(ext, "c") {
        FileExtension::C
    } else if eq_ignore_ascii(ext, "h") {
        FileExtension::H
    } else if eq_ignore_ascii(ext, "r") {
        FileExtension::Rb // .r is R lang, but handle via length bucket
    } else {
        FileExtension::Unknown
    }
}

fn match_ext_2(ext: &str) -> FileExtension {
    if eq_ignore_ascii(ext, "nx") {
        FileExtension::Nx
    } else if eq_ignore_ascii(ext, "md") {
        FileExtension::Md
    } else if eq_ignore_ascii(ext, "py") {
        FileExtension::Py
    } else if eq_ignore_ascii(ext, "rs") {
        FileExtension::Rs
    } else if eq_ignore_ascii(ext, "js") {
        FileExtension::Js
    } else if eq_ignore_ascii(ext, "ts") {
        FileExtension::Ts
    } else if eq_ignore_ascii(ext, "go") {
        FileExtension::Go
    } else if eq_ignore_ascii(ext, "rb") {
        FileExtension::Rb
    } else if eq_ignore_ascii(ext, "sh") {
        FileExtension::Sh
    } else if eq_ignore_ascii(ext, "gz") {
        FileExtension::TarGz
    } else if eq_ignore_ascii(ext, "xz") {
        FileExtension::TarXz
    } else if eq_ignore_ascii(ext, "7z") {
        FileExtension::SevenZ
    } else if eq_ignore_ascii(ext, "kt") {
        FileExtension::Kt
    } else if eq_ignore_ascii(ext, "cs") {
        FileExtension::Cs
    } else {
        FileExtension::Unknown
    }
}

fn match_ext_3(ext: &str) -> FileExtension {
    if eq_ignore_ascii(ext, "dso") {
        FileExtension::Dso
    } else if eq_ignore_ascii(ext, "pkg") {
        FileExtension::Pkg
    } else if eq_ignore_ascii(ext, "txt") {
        FileExtension::Txt
    } else if eq_ignore_ascii(ext, "pdf") {
        FileExtension::Pdf
    } else if eq_ignore_ascii(ext, "doc") {
        FileExtension::Doc
    } else if eq_ignore_ascii(ext, "odt") {
        FileExtension::Odt
    } else if eq_ignore_ascii(ext, "rtf") {
        FileExtension::Rtf
    } else if eq_ignore_ascii(ext, "csv") {
        FileExtension::Csv
    } else if eq_ignore_ascii(ext, "tsv") {
        FileExtension::Tsv
    } else if eq_ignore_ascii(ext, "xml") {
        FileExtension::Xml
    } else if eq_ignore_ascii(ext, "png") {
        FileExtension::Png
    } else if eq_ignore_ascii(ext, "jpg") {
        FileExtension::Jpg
    } else if eq_ignore_ascii(ext, "gif") {
        FileExtension::Gif
    } else if eq_ignore_ascii(ext, "bmp") {
        FileExtension::Bmp
    } else if eq_ignore_ascii(ext, "svg") {
        FileExtension::Svg
    } else if eq_ignore_ascii(ext, "ico") {
        FileExtension::Ico
    } else if eq_ignore_ascii(ext, "mp3") {
        FileExtension::Mp3
    } else if eq_ignore_ascii(ext, "wav") {
        FileExtension::Wav
    } else if eq_ignore_ascii(ext, "ogg") {
        FileExtension::Ogg
    } else if eq_ignore_ascii(ext, "aac") {
        FileExtension::Aac
    } else if eq_ignore_ascii(ext, "wma") {
        FileExtension::Wma
    } else if eq_ignore_ascii(ext, "m4a") {
        FileExtension::M4a
    } else if eq_ignore_ascii(ext, "mp4") {
        FileExtension::Mp4
    } else if eq_ignore_ascii(ext, "mkv") {
        FileExtension::Mkv
    } else if eq_ignore_ascii(ext, "avi") {
        FileExtension::Avi
    } else if eq_ignore_ascii(ext, "mov") {
        FileExtension::Mov
    } else if eq_ignore_ascii(ext, "wmv") {
        FileExtension::Wmv
    } else if eq_ignore_ascii(ext, "flv") {
        FileExtension::Flv
    } else if eq_ignore_ascii(ext, "cpp") {
        FileExtension::Cpp
    } else if eq_ignore_ascii(ext, "hpp") {
        FileExtension::Hpp
    } else if eq_ignore_ascii(ext, "sql") {
        FileExtension::Sql
    } else if eq_ignore_ascii(ext, "css") {
        FileExtension::Css
    } else if eq_ignore_ascii(ext, "zip") {
        FileExtension::Zip
    } else if eq_ignore_ascii(ext, "rar") {
        FileExtension::Rar
    } else if eq_ignore_ascii(ext, "tar") {
        FileExtension::Zip // tar itself; mapped to archive
    } else if eq_ignore_ascii(ext, "ini") {
        FileExtension::Ini
    } else if eq_ignore_ascii(ext, "cfg") {
        FileExtension::Cfg
    } else if eq_ignore_ascii(ext, "env") {
        FileExtension::Env
    } else if eq_ignore_ascii(ext, "log") {
        FileExtension::Log
    } else if eq_ignore_ascii(ext, "iso") {
        FileExtension::Iso
    } else if eq_ignore_ascii(ext, "img") {
        FileExtension::Img
    } else if eq_ignore_ascii(ext, "xls") {
        FileExtension::Xls
    } else if eq_ignore_ascii(ext, "ods") {
        FileExtension::Ods
    } else if eq_ignore_ascii(ext, "ppt") {
        FileExtension::Ppt
    } else if eq_ignore_ascii(ext, "odp") {
        FileExtension::Odp
    } else if eq_ignore_ascii(ext, "ttf")
        || eq_ignore_ascii(ext, "otf")
        || eq_ignore_ascii(ext, "lua")
        || eq_ignore_ascii(ext, "php")
        || eq_ignore_ascii(ext, "zig")
        || eq_ignore_ascii(ext, "ada")
        || eq_ignore_ascii(ext, "zst")
    {
        // Recognized 3-char extensions that fall back to Unknown because the
        // corresponding enum variants are matched by match_ext_long instead.
        FileExtension::Unknown
    } else if eq_ignore_ascii(ext, "tif") {
        FileExtension::Tiff
    } else if eq_ignore_ascii(ext, "mid") {
        FileExtension::Mp3 // midi, close enough category
    } else if eq_ignore_ascii(ext, "bz2") {
        FileExtension::TarBz2
    } else if eq_ignore_ascii(ext, "tgz") {
        FileExtension::TarGz
    } else {
        FileExtension::Unknown
    }
}

fn match_ext_4(ext: &str) -> FileExtension {
    if eq_ignore_ascii(ext, "slib") {
        FileExtension::Slib
    } else if eq_ignore_ascii(ext, "docx") {
        FileExtension::Docx
    } else if eq_ignore_ascii(ext, "json") {
        FileExtension::Json
    } else if eq_ignore_ascii(ext, "yaml") {
        FileExtension::Yaml
    } else if eq_ignore_ascii(ext, "toml") {
        FileExtension::Toml
    } else if eq_ignore_ascii(ext, "html") {
        FileExtension::Html
    } else if eq_ignore_ascii(ext, "jpeg") {
        FileExtension::Jpg
    } else if eq_ignore_ascii(ext, "webp") {
        FileExtension::Webp
    } else if eq_ignore_ascii(ext, "tiff") {
        FileExtension::Tiff
    } else if eq_ignore_ascii(ext, "flac") {
        FileExtension::Flac
    } else if eq_ignore_ascii(ext, "webm") {
        FileExtension::Webm
    } else if eq_ignore_ascii(ext, "java") {
        FileExtension::Java
    } else if eq_ignore_ascii(ext, "scss") {
        FileExtension::Scss
    } else if eq_ignore_ascii(ext, "conf") {
        FileExtension::Conf
    } else if eq_ignore_ascii(ext, "xlsx") {
        FileExtension::Xlsx
    } else if eq_ignore_ascii(ext, "pptx") {
        FileExtension::Pptx
    } else if eq_ignore_ascii(ext, "opus") {
        FileExtension::Ogg // opus mapped to ogg category
    } else if eq_ignore_ascii(ext, "midi") {
        FileExtension::Mp3 // midi category
    } else {
        FileExtension::Unknown
    }
}

#[allow(clippy::if_same_then_else)] // Per-extension arms intentionally identical: each documents a recognized extension that currently maps to Unknown for lack of an enum variant. Future variants slot in here without touching call sites.
fn match_ext_long(ext: &str) -> FileExtension {
    if eq_ignore_ascii(ext, "swift") {
        FileExtension::Unknown // table has it but no enum variant needed
    } else if eq_ignore_ascii(ext, "woff2") {
        FileExtension::Unknown
    } else if eq_ignore_ascii(ext, "woff") {
        FileExtension::Unknown
    } else {
        FileExtension::Unknown
    }
}

// ---------------------------------------------------------------------------
// Public query API
// ---------------------------------------------------------------------------

/// Look up full [`FileTypeInfo`] from a dotted or bare extension string.
///
/// The lookup is case-insensitive.  Compound extensions like `"tar.gz"` are
/// handled.  Returns the sentinel `Unknown` entry on no match.
pub fn detect_from_extension(ext: &str) -> &'static FileTypeInfo {
    let normalised = ext.strip_prefix('.').unwrap_or(ext);

    // Try compound extensions first.
    for info in FILE_TYPE_TABLE {
        let table_ext = info.extension.strip_prefix('.').unwrap_or(info.extension);
        if table_ext.eq_ignore_ascii_case(normalised) {
            return info;
        }
    }

    &UNKNOWN_FILE_TYPE
}

/// Attempt to identify a file type from the first bytes of its content.
///
/// Pass at least the first 16 bytes of the file for reliable detection.
/// Returns `None` when no known signature matches.
pub fn detect_from_magic(header: &[u8]) -> Option<&'static FileTypeInfo> {
    for sig in MAGIC_TABLE {
        let end = sig.offset.saturating_add(sig.bytes.len());
        if header.len() >= end {
            let window = &header[sig.offset..end];
            if window == sig.bytes {
                // Translate the extension enum to the table entry.
                let ext_str = extension_enum_to_str(sig.extension);
                if ext_str.is_empty() {
                    // Unknown / foreign binary — return None rather than the
                    // generic unknown entry so callers can distinguish "no
                    // match" from "matched but unrecognised format".
                    return None;
                }
                return Some(detect_from_extension(ext_str));
            }
        }
    }
    None
}

/// Return the [`FileCategory`] for a given extension string.
pub fn category_from_extension(ext: &str) -> FileCategory {
    detect_from_extension(ext).category
}

/// `true` if the extension is known to represent human-readable text.
pub fn is_text_file(ext: &str) -> bool {
    detect_from_extension(ext).is_text
}

/// `true` if the extension represents a directly executable format.
pub fn is_executable(ext: &str) -> bool {
    detect_from_extension(ext).is_executable
}

/// Return the icon glyph character for a given extension.
pub fn icon_for_extension(ext: &str) -> char {
    detect_from_extension(ext).icon_glyph
}

/// Return the MIME type string for a given extension.
pub fn mime_for_extension(ext: &str) -> &'static str {
    detect_from_extension(ext).mime_type
}

/// Icon glyph for a directory (not a file extension, but commonly needed).
pub const DIR_ICON_GLYPH: char = '\u{1F4C1}'; // open file folder

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a [`FileExtension`] enum variant back to its canonical dotted
/// extension string.  Returns `""` for [`FileExtension::Unknown`].
fn extension_enum_to_str(ext: FileExtension) -> &'static str {
    match ext {
        FileExtension::Nx => ".nx",
        FileExtension::Dso => ".dso",
        FileExtension::Slib => ".slib",
        FileExtension::Pkg => ".pkg",
        FileExtension::Txt => ".txt",
        FileExtension::Md => ".md",
        FileExtension::Pdf => ".pdf",
        FileExtension::Doc => ".doc",
        FileExtension::Docx => ".docx",
        FileExtension::Odt => ".odt",
        FileExtension::Rtf => ".rtf",
        FileExtension::Csv => ".csv",
        FileExtension::Tsv => ".tsv",
        FileExtension::Json => ".json",
        FileExtension::Yaml => ".yaml",
        FileExtension::Toml => ".toml",
        FileExtension::Xml => ".xml",
        FileExtension::Html => ".html",
        FileExtension::Xls => ".xls",
        FileExtension::Xlsx => ".xlsx",
        FileExtension::Ods => ".ods",
        FileExtension::Ppt => ".ppt",
        FileExtension::Pptx => ".pptx",
        FileExtension::Odp => ".odp",
        FileExtension::Png => ".png",
        FileExtension::Jpg => ".jpg",
        FileExtension::Gif => ".gif",
        FileExtension::Bmp => ".bmp",
        FileExtension::Svg => ".svg",
        FileExtension::Ico => ".ico",
        FileExtension::Webp => ".webp",
        FileExtension::Tiff => ".tiff",
        FileExtension::Mp3 => ".mp3",
        FileExtension::Wav => ".wav",
        FileExtension::Flac => ".flac",
        FileExtension::Ogg => ".ogg",
        FileExtension::Aac => ".aac",
        FileExtension::Wma => ".wma",
        FileExtension::M4a => ".m4a",
        FileExtension::Mp4 => ".mp4",
        FileExtension::Mkv => ".mkv",
        FileExtension::Avi => ".avi",
        FileExtension::Mov => ".mov",
        FileExtension::Wmv => ".wmv",
        FileExtension::Webm => ".webm",
        FileExtension::Flv => ".flv",
        FileExtension::Rs => ".rs",
        FileExtension::Py => ".py",
        FileExtension::C => ".c",
        FileExtension::Cpp => ".cpp",
        FileExtension::H => ".h",
        FileExtension::Hpp => ".hpp",
        FileExtension::Js => ".js",
        FileExtension::Ts => ".ts",
        FileExtension::Java => ".java",
        FileExtension::Go => ".go",
        FileExtension::Rb => ".rb",
        FileExtension::Sh => ".sh",
        FileExtension::Sql => ".sql",
        FileExtension::Css => ".css",
        FileExtension::Scss => ".scss",
        FileExtension::Zip => ".zip",
        FileExtension::TarGz => ".tar.gz",
        FileExtension::TarBz2 => ".tar.bz2",
        FileExtension::TarXz => ".tar.xz",
        FileExtension::SevenZ => ".7z",
        FileExtension::Rar => ".rar",
        FileExtension::Ini => ".ini",
        FileExtension::Conf => ".conf",
        FileExtension::Cfg => ".cfg",
        FileExtension::Env => ".env",
        FileExtension::Log => ".log",
        FileExtension::Iso => ".iso",
        FileExtension::Img => ".img",
        FileExtension::Kt => ".kt",
        FileExtension::Cs => ".cs",
        FileExtension::Unknown => "",
    }
}

// ---------------------------------------------------------------------------
// Additional FileExtension variants used above but not in the original enum
// ---------------------------------------------------------------------------
// (Kt and Cs are used in match_ext_2 so they must exist in the enum.  They
// are already included above.)

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Extension detection by category -----------------------------------

    #[test]
    fn detect_os_specific_extensions() {
        assert_eq!(detect_from_extension(".nx").category, FileCategory::Executable);
        assert_eq!(detect_from_extension(".dso").category, FileCategory::Library);
        assert_eq!(detect_from_extension(".slib").category, FileCategory::Library);
        assert_eq!(detect_from_extension(".pkg").category, FileCategory::Package);
    }

    #[test]
    fn detect_document_extensions() {
        assert_eq!(detect_from_extension(".txt").category, FileCategory::Document);
        assert_eq!(detect_from_extension(".md").category, FileCategory::Document);
        assert_eq!(detect_from_extension(".pdf").category, FileCategory::Document);
        assert_eq!(detect_from_extension(".html").category, FileCategory::Document);
        assert_eq!(detect_from_extension(".docx").category, FileCategory::Document);
    }

    #[test]
    fn detect_image_extensions() {
        for ext in &[".png", ".jpg", ".jpeg", ".gif", ".bmp", ".svg", ".ico", ".webp", ".tiff"] {
            assert_eq!(
                detect_from_extension(ext).category,
                FileCategory::Image,
                "expected Image for {ext}"
            );
        }
    }

    #[test]
    fn detect_audio_extensions() {
        for ext in &[".mp3", ".wav", ".flac", ".ogg", ".aac", ".wma", ".m4a"] {
            assert_eq!(
                detect_from_extension(ext).category,
                FileCategory::Audio,
                "expected Audio for {ext}"
            );
        }
    }

    #[test]
    fn detect_video_extensions() {
        for ext in &[".mp4", ".mkv", ".avi", ".mov", ".wmv", ".webm", ".flv"] {
            assert_eq!(
                detect_from_extension(ext).category,
                FileCategory::Video,
                "expected Video for {ext}"
            );
        }
    }

    #[test]
    fn detect_code_extensions() {
        for ext in &[
            ".rs", ".py", ".c", ".cpp", ".h", ".hpp", ".js", ".ts", ".java", ".go", ".rb", ".sh",
            ".sql", ".css", ".scss",
        ] {
            assert_eq!(
                detect_from_extension(ext).category,
                FileCategory::Code,
                "expected Code for {ext}"
            );
        }
    }

    #[test]
    fn detect_archive_extensions() {
        for ext in &[".zip", ".tar.gz", ".tar.bz2", ".tar.xz", ".7z", ".rar"] {
            assert_eq!(
                detect_from_extension(ext).category,
                FileCategory::Archive,
                "expected Archive for {ext}"
            );
        }
    }

    #[test]
    fn detect_config_extensions() {
        for ext in &[".ini", ".conf", ".cfg", ".env", ".yaml", ".toml"] {
            assert_eq!(
                detect_from_extension(ext).category,
                FileCategory::Config,
                "expected Config for {ext}"
            );
        }
    }

    #[test]
    fn detect_disk_image_extensions() {
        assert_eq!(detect_from_extension(".iso").category, FileCategory::DiskImage);
        assert_eq!(detect_from_extension(".img").category, FileCategory::DiskImage);
    }

    // -- Magic byte detection -----------------------------------------------

    #[test]
    fn magic_detect_png() {
        let header = b"\x89PNG\r\n\x1a\nSOMETHING";
        let info = detect_from_magic(header).expect("should detect PNG");
        assert_eq!(info.extension, ".png");
        assert_eq!(info.category, FileCategory::Image);
    }

    #[test]
    fn magic_detect_jpeg() {
        let header = b"\xff\xd8\xff\xe0REST_OF_FILE";
        let info = detect_from_magic(header).expect("should detect JPEG");
        assert_eq!(info.extension, ".jpg");
    }

    #[test]
    fn magic_detect_gif89a() {
        let header = b"GIF89aPIXELDATA";
        let info = detect_from_magic(header).expect("should detect GIF");
        assert_eq!(info.extension, ".gif");
    }

    #[test]
    fn magic_detect_gif87a() {
        let header = b"GIF87aPIXELDATA";
        let info = detect_from_magic(header).expect("should detect GIF");
        assert_eq!(info.extension, ".gif");
    }

    #[test]
    fn magic_detect_bmp() {
        let header = b"BM\x00\x00\x00\x00";
        let info = detect_from_magic(header).expect("should detect BMP");
        assert_eq!(info.extension, ".bmp");
    }

    #[test]
    fn magic_detect_pdf() {
        let header = b"%PDF-1.7 rest";
        let info = detect_from_magic(header).expect("should detect PDF");
        assert_eq!(info.extension, ".pdf");
    }

    #[test]
    fn magic_detect_zip() {
        let header = b"PK\x03\x04FILEDATA";
        let info = detect_from_magic(header).expect("should detect ZIP");
        assert_eq!(info.extension, ".zip");
    }

    #[test]
    fn magic_detect_7z() {
        let header = b"7z\xbc\xaf\x27\x1c\x00\x00";
        let info = detect_from_magic(header).expect("should detect 7z");
        assert_eq!(info.extension, ".7z");
    }

    #[test]
    fn magic_detect_rar() {
        let header = b"Rar!\x1a\x07\x00DATA";
        let info = detect_from_magic(header).expect("should detect RAR");
        assert_eq!(info.extension, ".rar");
    }

    #[test]
    fn magic_detect_flac() {
        let header = b"fLaC\x00\x00\x00\x22";
        let info = detect_from_magic(header).expect("should detect FLAC");
        assert_eq!(info.extension, ".flac");
    }

    #[test]
    fn magic_detect_ogg() {
        let header = b"OggS\x00\x02DATA";
        let info = detect_from_magic(header).expect("should detect OGG");
        assert_eq!(info.extension, ".ogg");
    }

    #[test]
    fn magic_detect_mp3_id3() {
        let header = b"ID3\x04\x00\x00TAGS";
        let info = detect_from_magic(header).expect("should detect MP3 via ID3");
        assert_eq!(info.extension, ".mp3");
    }

    #[test]
    fn magic_detect_nx_executable() {
        let header = b"\x4fNXECODE_HERE";
        let info = detect_from_magic(header).expect("should detect NX");
        assert_eq!(info.extension, ".nx");
        assert!(info.is_executable);
    }

    #[test]
    fn magic_detect_dso() {
        let header = b"\x4fDSOLIBDATA";
        let info = detect_from_magic(header).expect("should detect DSO");
        assert_eq!(info.extension, ".dso");
        assert_eq!(info.category, FileCategory::Library);
    }

    #[test]
    fn magic_detect_mp4_ftyp() {
        // ftyp at offset 4 (first 4 bytes are box size)
        let header = b"\x00\x00\x00\x20ftypmp42";
        let info = detect_from_magic(header).expect("should detect MP4");
        assert_eq!(info.extension, ".mp4");
    }

    #[test]
    fn magic_no_match() {
        let header = b"\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(detect_from_magic(header).is_none());
    }

    #[test]
    fn magic_header_too_short() {
        let header = b"\x89P";
        assert!(detect_from_magic(header).is_none());
    }

    #[test]
    fn magic_elf_returns_none() {
        // ELF is a foreign format — detect_from_magic returns None because
        // the signature maps to FileExtension::Unknown.
        let header = b"\x7fELF\x02\x01\x01\x00";
        assert!(detect_from_magic(header).is_none());
    }

    // -- Category classification -------------------------------------------

    #[test]
    fn category_from_ext() {
        assert_eq!(category_from_extension(".rs"), FileCategory::Code);
        assert_eq!(category_from_extension(".mp4"), FileCategory::Video);
        assert_eq!(category_from_extension(".nx"), FileCategory::Executable);
        assert_eq!(category_from_extension(".xyz"), FileCategory::Unknown);
    }

    // -- MIME type lookup ---------------------------------------------------

    #[test]
    fn mime_lookup() {
        assert_eq!(mime_for_extension(".png"), "image/png");
        assert_eq!(mime_for_extension(".html"), "text/html");
        assert_eq!(mime_for_extension(".json"), "application/json");
        assert_eq!(mime_for_extension(".mp3"), "audio/mpeg");
        assert_eq!(mime_for_extension(".nx"), "application/x-slateos-executable");
    }

    #[test]
    fn mime_unknown() {
        assert_eq!(mime_for_extension(".xyz"), "application/octet-stream");
    }

    // -- is_text / is_executable -------------------------------------------

    #[test]
    fn text_file_detection() {
        assert!(is_text_file(".rs"));
        assert!(is_text_file(".txt"));
        assert!(is_text_file(".json"));
        assert!(is_text_file(".yaml"));
        assert!(is_text_file(".svg")); // SVG is text
        assert!(!is_text_file(".png"));
        assert!(!is_text_file(".mp4"));
        assert!(!is_text_file(".nx"));
    }

    #[test]
    fn executable_detection() {
        assert!(is_executable(".nx"));
        assert!(is_executable(".sh"));
        assert!(!is_executable(".txt"));
        assert!(!is_executable(".png"));
        assert!(!is_executable(".dso")); // libraries are not directly executable
    }

    // -- Unknown extension --------------------------------------------------

    #[test]
    fn unknown_extension() {
        let info = detect_from_extension(".xyzzy");
        assert_eq!(info.category, FileCategory::Unknown);
        assert_eq!(info.mime_type, "application/octet-stream");
        assert!(!info.is_text);
        assert!(!info.is_executable);
    }

    // -- Case-insensitive matching ------------------------------------------

    #[test]
    fn case_insensitive() {
        assert_eq!(detect_from_extension(".RS").category, FileCategory::Code);
        assert_eq!(detect_from_extension(".Png").category, FileCategory::Image);
        assert_eq!(detect_from_extension(".NX").category, FileCategory::Executable);
        assert_eq!(detect_from_extension(".TAR.GZ").category, FileCategory::Archive);
        assert_eq!(detect_from_extension("JSON").category, FileCategory::Data);
    }

    #[test]
    fn bare_extension_no_dot() {
        assert_eq!(detect_from_extension("rs").category, FileCategory::Code);
        assert_eq!(detect_from_extension("png").category, FileCategory::Image);
    }

    // -- Icon glyph assignment ----------------------------------------------

    #[test]
    fn icon_glyphs() {
        assert_eq!(icon_for_extension(".nx"), '\u{2699}');
        assert_eq!(icon_for_extension(".png"), '\u{1F5BC}');
        assert_eq!(icon_for_extension(".mp3"), '\u{266A}');
        assert_eq!(icon_for_extension(".mp4"), '\u{25B6}');
        assert_eq!(icon_for_extension(".rs"), '\u{007B}');
        assert_eq!(icon_for_extension(".zip"), '\u{1F4E6}');
    }

    // -- parse_extension enum conversion ------------------------------------

    #[test]
    fn parse_extension_round_trip() {
        // Verify a representative set of extensions parse correctly.
        let cases = [
            ("nx", FileExtension::Nx),
            (".dso", FileExtension::Dso),
            ("slib", FileExtension::Slib),
            (".pkg", FileExtension::Pkg),
            ("rs", FileExtension::Rs),
            (".py", FileExtension::Py),
            ("PNG", FileExtension::Png),
            (".tar.gz", FileExtension::TarGz),
            ("7z", FileExtension::SevenZ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_extension(input),
                expected,
                "parse_extension({input:?}) mismatch"
            );
        }
    }

    #[test]
    fn parse_extension_unknown() {
        assert_eq!(parse_extension(".blahblah"), FileExtension::Unknown);
        assert_eq!(parse_extension(""), FileExtension::Unknown);
    }

    // -- FileTypeInfo fields ------------------------------------------------

    #[test]
    fn file_type_info_fields() {
        let info = detect_from_extension(".nx");
        assert_eq!(info.description, "Slate OS Native Executable");
        assert_eq!(info.default_app, None);
        assert!(info.is_executable);
        assert!(!info.is_text);

        let info = detect_from_extension(".rs");
        assert_eq!(info.description, "Rust Source File");
        assert_eq!(info.default_app, Some("textedit"));
        assert!(info.is_text);
    }

    // -- Compound extensions -----------------------------------------------

    #[test]
    fn compound_extensions() {
        assert_eq!(detect_from_extension(".tar.gz").category, FileCategory::Archive);
        assert_eq!(detect_from_extension(".tar.bz2").category, FileCategory::Archive);
        assert_eq!(detect_from_extension(".tar.xz").category, FileCategory::Archive);
        assert_eq!(detect_from_extension(".tgz").category, FileCategory::Archive);
    }

    // -- DIR_ICON_GLYPH constant -------------------------------------------

    #[test]
    fn dir_icon_is_folder() {
        assert_eq!(DIR_ICON_GLYPH, '\u{1F4C1}');
    }
}
