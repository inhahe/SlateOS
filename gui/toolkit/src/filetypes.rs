//! File type definitions, detection, and categorisation for SlateOS.
//!
//! Provides a centralised registry of known file extensions, magic-byte
//! signatures, MIME types, icon glyphs, and category tags.  Every GUI
//! component that needs to display, open, or classify a file should go
//! through this module rather than hard-coding extension lists.

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
}

// ---------------------------------------------------------------------------
// Compile-time info table
// ---------------------------------------------------------------------------

/// Master lookup table.  Sorted by extension for binary-search, but we also
/// use a linear scan with case-folding so order is not critical for
/// correctness.
const FILE_TYPE_TABLE: &[FileTypeInfo] = &[
    // -- Added 2026-09-16 ---------------------------------------------------
    // Second batch, same day. Two archives missed on the first pass, and two
    // formats this system genuinely runs: it has a POSIX layer, so an ELF
    // binary and a shared object are ours rather than foreign. `.so` is
    // `Library` and not `Executable` because it is loaded, not started --
    // `is_executable: false` is the same distinction.
    FileTypeInfo {
        extension: ".cab",
        description: "Cabinet Archive",
        mime_type: "application/vnd.ms-cab-compressed",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".lz4",
        description: "LZ4 Compressed File",
        mime_type: "application/x-lz4",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".elf",
        description: "ELF Executable",
        mime_type: "application/x-executable",
        category: FileCategory::Executable,
        icon_glyph: '\u{2699}',
        is_text: false,
        is_executable: true,
    },
    FileTypeInfo {
        extension: ".so",
        description: "Shared Object",
        mime_type: "application/x-sharedlib",
        category: FileCategory::Library,
        icon_glyph: '\u{2699}',
        is_text: false,
        is_executable: false,
    },
    //
    // Twelve formats `apps/filesearch` classified and this table did not, found
    // by diffing the two lists (known-issues
    // TD-C-FOUR-PLACES-DECIDE-WHAT-KIND-OF-FILE-SOMETHING-IS). Only the group
    // that needed no decision is here: media and text formats this system has
    // every reason to recognise. Deliberately still absent are `exe`, `dll`,
    // `msi`, `app` and `dylib` -- foreign executables this OS cannot run, where
    // listing them would have the table claim a kind for something nothing can
    // open, and whether that is wanted is a question nobody has answered.
    // `raw` is absent too: it names camera images and raw byte dumps equally,
    // so any single description would be a guess.
    FileTypeInfo {
        extension: ".mpg",
        description: "MPEG Video",
        mime_type: "video/mpeg",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".mpeg",
        description: "MPEG Video",
        mime_type: "video/mpeg",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".m4v",
        description: "MPEG-4 Video",
        mime_type: "video/x-m4v",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".vob",
        description: "DVD Video Object",
        mime_type: "video/mpeg",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".psd",
        description: "Photoshop Document",
        mime_type: "image/vnd.adobe.photoshop",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".yml",
        description: "YAML Document",
        mime_type: "application/x-yaml",
        category: FileCategory::Config,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tex",
        description: "LaTeX Document",
        mime_type: "text/x-tex",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".bash",
        description: "Bash Script",
        mime_type: "application/x-shellscript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".zsh",
        description: "Zsh Script",
        mime_type: "application/x-shellscript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".fish",
        description: "Fish Script",
        mime_type: "application/x-shellscript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".ps1",
        description: "PowerShell Script",
        mime_type: "application/x-powershell",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".properties",
        description: "Java Properties",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
    },
    // -- OS-specific --------------------------------------------------------
    FileTypeInfo {
        extension: ".nx",
        description: "Slate OS Native Executable",
        mime_type: "application/x-slateos-executable",
        category: FileCategory::Executable,
        icon_glyph: '\u{2699}', // gear
        is_text: false,
        is_executable: true,
    },
    FileTypeInfo {
        extension: ".dso",
        description: "Dynamic Shared Object",
        mime_type: "application/x-slateos-shared-library",
        category: FileCategory::Library,
        icon_glyph: '\u{1F4E6}', // package
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".slib",
        description: "Static Library",
        mime_type: "application/x-slateos-static-library",
        category: FileCategory::Library,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".pkg",
        description: "Slate OS Package Archive",
        mime_type: "application/x-slateos-package",
        category: FileCategory::Package,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".md",
        description: "Markdown Document",
        mime_type: "text/markdown",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".pdf",
        description: "PDF Document",
        mime_type: "application/pdf",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".doc",
        description: "Microsoft Word Document (Legacy)",
        mime_type: "application/msword",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".docx",
        description: "Microsoft Word Document",
        mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".odt",
        description: "OpenDocument Text",
        mime_type: "application/vnd.oasis.opendocument.text",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".rtf",
        description: "Rich Text Format",
        mime_type: "application/rtf",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".csv",
        description: "Comma-Separated Values",
        mime_type: "text/csv",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}', // bar chart
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tsv",
        description: "Tab-Separated Values",
        mime_type: "text/tab-separated-values",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".json",
        description: "JSON Data",
        mime_type: "application/json",
        category: FileCategory::Data,
        icon_glyph: '\u{007B}', // {
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".yaml",
        description: "YAML Document",
        mime_type: "application/x-yaml",
        category: FileCategory::Config,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".toml",
        description: "TOML Configuration",
        mime_type: "application/toml",
        category: FileCategory::Config,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".xml",
        description: "XML Document",
        mime_type: "application/xml",
        category: FileCategory::Data,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".html",
        description: "HTML Document",
        mime_type: "text/html",
        category: FileCategory::Document,
        icon_glyph: '\u{1F310}', // globe with meridians
        is_text: true,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".xlsx",
        description: "Microsoft Excel Spreadsheet",
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".ods",
        description: "OpenDocument Spreadsheet",
        mime_type: "application/vnd.oasis.opendocument.spreadsheet",
        category: FileCategory::Spreadsheet,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".ppt",
        description: "Microsoft PowerPoint (Legacy)",
        mime_type: "application/vnd.ms-powerpoint",
        category: FileCategory::Presentation,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".pptx",
        description: "Microsoft PowerPoint Presentation",
        mime_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        category: FileCategory::Presentation,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".odp",
        description: "OpenDocument Presentation",
        mime_type: "application/vnd.oasis.opendocument.presentation",
        category: FileCategory::Presentation,
        icon_glyph: '\u{1F4CA}',
        is_text: false,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".jpg",
        description: "JPEG Image",
        mime_type: "image/jpeg",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".jpeg",
        description: "JPEG Image",
        mime_type: "image/jpeg",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".gif",
        description: "GIF Image",
        mime_type: "image/gif",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".bmp",
        description: "BMP Image",
        mime_type: "image/bmp",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".svg",
        description: "SVG Image",
        mime_type: "image/svg+xml",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".ico",
        description: "Icon Image",
        mime_type: "image/x-icon",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".webp",
        description: "WebP Image",
        mime_type: "image/webp",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    // AVIF, and the HEIF family it is built on: pictures in an ISO Base Media
    // container, the same `ftyp` box an MP4 opens with. `imagecodec` decodes
    // AVIF (stills, grids and animated sequences, design-decisions §1333);
    // nothing here decodes HEIC yet, but a HEIC is still a picture, and
    // calling it one is what lets whatever opens it say so honestly.
    FileTypeInfo {
        extension: ".avif",
        description: "AVIF Image",
        mime_type: "image/avif",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".heic",
        description: "HEIC Image",
        mime_type: "image/heic",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".heif",
        description: "HEIF Image",
        mime_type: "image/heif",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tiff",
        description: "TIFF Image",
        mime_type: "image/tiff",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tif",
        description: "TIFF Image",
        mime_type: "image/tiff",
        category: FileCategory::Image,
        icon_glyph: '\u{1F5BC}',
        is_text: false,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".wav",
        description: "WAV Audio",
        mime_type: "audio/wav",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".flac",
        description: "FLAC Audio",
        mime_type: "audio/flac",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".ogg",
        description: "Ogg Vorbis Audio",
        mime_type: "audio/ogg",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".aac",
        description: "AAC Audio",
        mime_type: "audio/aac",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".wma",
        description: "Windows Media Audio",
        mime_type: "audio/x-ms-wma",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".m4a",
        description: "MPEG-4 Audio",
        mime_type: "audio/mp4",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".opus",
        description: "Opus Audio",
        mime_type: "audio/opus",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".midi",
        description: "MIDI Audio",
        mime_type: "audio/midi",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".mid",
        description: "MIDI Audio",
        mime_type: "audio/midi",
        category: FileCategory::Audio,
        icon_glyph: '\u{266A}',
        is_text: false,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".mkv",
        description: "Matroska Video",
        mime_type: "video/x-matroska",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".avi",
        description: "AVI Video",
        mime_type: "video/x-msvideo",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".mov",
        description: "QuickTime Video",
        mime_type: "video/quicktime",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".wmv",
        description: "Windows Media Video",
        mime_type: "video/x-ms-wmv",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".webm",
        description: "WebM Video",
        mime_type: "video/webm",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".flv",
        description: "Flash Video",
        mime_type: "video/x-flv",
        category: FileCategory::Video,
        icon_glyph: '\u{25B6}',
        is_text: false,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".py",
        description: "Python Source File",
        mime_type: "text/x-python",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".c",
        description: "C Source File",
        mime_type: "text/x-c",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".cpp",
        description: "C++ Source File",
        mime_type: "text/x-c++",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".h",
        description: "C/C++ Header File",
        mime_type: "text/x-c",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".hpp",
        description: "C++ Header File",
        mime_type: "text/x-c++",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".js",
        description: "JavaScript Source File",
        mime_type: "text/javascript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".ts",
        description: "TypeScript Source File",
        mime_type: "text/typescript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".java",
        description: "Java Source File",
        mime_type: "text/x-java",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".go",
        description: "Go Source File",
        mime_type: "text/x-go",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".rb",
        description: "Ruby Source File",
        mime_type: "text/x-ruby",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".sh",
        description: "Shell Script",
        mime_type: "application/x-shellscript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: true,
    },
    FileTypeInfo {
        extension: ".sql",
        description: "SQL Script",
        mime_type: "application/sql",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".css",
        description: "CSS Stylesheet",
        mime_type: "text/css",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".scss",
        description: "SCSS Stylesheet",
        mime_type: "text/x-scss",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".lua",
        description: "Lua Source File",
        mime_type: "text/x-lua",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".php",
        description: "PHP Source File",
        mime_type: "text/x-php",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".swift",
        description: "Swift Source File",
        mime_type: "text/x-swift",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".kt",
        description: "Kotlin Source File",
        mime_type: "text/x-kotlin",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".cs",
        description: "C# Source File",
        mime_type: "text/x-csharp",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".r",
        description: "R Source File",
        mime_type: "text/x-r",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".zig",
        description: "Zig Source File",
        mime_type: "text/x-zig",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".ada",
        description: "Ada Source File",
        mime_type: "text/x-ada",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".tar.gz",
        description: "Gzip-Compressed Tar Archive",
        mime_type: "application/gzip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tgz",
        description: "Gzip-Compressed Tar Archive",
        mime_type: "application/gzip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tar.bz2",
        description: "Bzip2-Compressed Tar Archive",
        mime_type: "application/x-bzip2",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tar.xz",
        description: "XZ-Compressed Tar Archive",
        mime_type: "application/x-xz",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".7z",
        description: "7-Zip Archive",
        mime_type: "application/x-7z-compressed",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".rar",
        description: "RAR Archive",
        mime_type: "application/vnd.rar",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".tar",
        description: "Tar Archive",
        mime_type: "application/x-tar",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".gz",
        description: "Gzip Compressed File",
        mime_type: "application/gzip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".bz2",
        description: "Bzip2 Compressed File",
        mime_type: "application/x-bzip2",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".xz",
        description: "XZ Compressed File",
        mime_type: "application/x-xz",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".zst",
        description: "Zstandard Compressed File",
        mime_type: "application/zstd",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".conf",
        description: "Configuration File",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".cfg",
        description: "Configuration File",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".env",
        description: "Environment Variables",
        mime_type: "text/plain",
        category: FileCategory::Config,
        icon_glyph: '\u{2699}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".log",
        description: "Log File",
        mime_type: "text/plain",
        category: FileCategory::Data,
        icon_glyph: '\u{1F4C3}', // page with curl
        is_text: true,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".img",
        description: "Disk Image",
        mime_type: "application/octet-stream",
        category: FileCategory::DiskImage,
        icon_glyph: '\u{1F4BF}',
        is_text: false,
        is_executable: false,
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
    },
    FileTypeInfo {
        extension: ".otf",
        description: "OpenType Font",
        mime_type: "font/otf",
        category: FileCategory::System,
        icon_glyph: '\u{0041}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".woff",
        description: "Web Open Font Format",
        mime_type: "font/woff",
        category: FileCategory::System,
        icon_glyph: '\u{0041}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".woff2",
        description: "Web Open Font Format 2",
        mime_type: "font/woff2",
        category: FileCategory::System,
        icon_glyph: '\u{0041}',
        is_text: false,
        is_executable: false,
    },
    // Carried from the kernel's `fs::mime` and `fs::filetype` when the
    // program lists became one (gui/programs/INVENTORY.md section 5).
    // The ones that are waiting on a decision are not here; see the test
    // `the_kernels_types_this_table_waits_to_decide_are_still_absent`. `.oga`
    // waits on `apps/fileassoc`, whose group test counts this table's audio
    // types by hand (requests/c-e-a-test-that-counts-the-toolkits-audio-types.md).
    FileTypeInfo {
        extension: ".bat",
        description: "Windows Batch File",
        mime_type: "text/x-msdos-batch",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".cmd",
        description: "Windows Command Script",
        mime_type: "text/x-msdos-batch",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".cc",
        description: "C++ Source File",
        mime_type: "text/x-c++",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".cxx",
        description: "C++ Source File",
        mime_type: "text/x-c++",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".hxx",
        description: "C++ Header File",
        mime_type: "text/x-c++",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".mjs",
        description: "JavaScript Module",
        mime_type: "text/javascript",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".psm1",
        description: "PowerShell Module",
        mime_type: "application/x-powershell",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".pyw",
        description: "Python Source File",
        mime_type: "text/x-python",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".diff",
        description: "Patch File",
        mime_type: "text/x-diff",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".patch",
        description: "Patch File",
        mime_type: "text/x-diff",
        category: FileCategory::Code,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".htm",
        description: "HTML Document",
        mime_type: "text/html",
        category: FileCategory::Document,
        icon_glyph: '\u{1F310}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".markdown",
        description: "Markdown Document",
        mime_type: "text/markdown",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".text",
        description: "Plain Text",
        mime_type: "text/plain",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".xsd",
        description: "XML Schema",
        mime_type: "application/xml",
        category: FileCategory::Data,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".xsl",
        description: "XSL Stylesheet",
        mime_type: "application/xml",
        category: FileCategory::Data,
        icon_glyph: '\u{007B}',
        is_text: true,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".cpio",
        description: "CPIO Archive",
        mime_type: "application/x-cpio",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".gzip",
        description: "Gzip Compressed File",
        mime_type: "application/gzip",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".zstd",
        description: "Zstandard Compressed File",
        mime_type: "application/zstd",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".jar",
        description: "Java Archive",
        mime_type: "application/java-archive",
        category: FileCategory::Archive,
        icon_glyph: '\u{1F4E6}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".a",
        description: "Static Library",
        mime_type: "application/x-archive",
        category: FileCategory::Library,
        icon_glyph: '\u{2699}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".lib",
        description: "Static Library",
        mime_type: "application/x-archive",
        category: FileCategory::Library,
        icon_glyph: '\u{2699}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".o",
        description: "Object File",
        mime_type: "application/x-object",
        category: FileCategory::Library,
        icon_glyph: '\u{2699}',
        is_text: false,
        is_executable: false,
    },
    FileTypeInfo {
        extension: ".epub",
        description: "EPUB E-book",
        mime_type: "application/epub+zip",
        category: FileCategory::Document,
        icon_glyph: '\u{1F4C4}',
        is_text: false,
        is_executable: false,
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
};

// ---------------------------------------------------------------------------
// Magic byte signatures
// ---------------------------------------------------------------------------

/// A magic-byte signature and the file type it identifies.
struct MagicSignature {
    /// Byte pattern that must appear at `offset`.
    bytes: &'static [u8],
    /// Offset from the start of the file where the pattern must appear.
    offset: usize,
    /// The row of [`FILE_TYPE_TABLE`] it identifies, by extension.
    ///
    /// An extension string rather than an enum: the enum that stood here
    /// lagged the table (a dozen rows had no variant), so a signature could
    /// only name what the enum happened to hold. The test
    /// `every_signature_names_a_row_of_the_table` holds each one to the table.
    extension: &'static str,
}

/// Known magic-byte patterns, checked in order: the more specific first where
/// two could match one file.
const MAGIC_TABLE: &[MagicSignature] = &[
    // Slate OS native formats
    MagicSignature {
        bytes: b"\x4fNXE",
        offset: 0,
        extension: ".nx",
    },
    MagicSignature {
        bytes: b"\x4fDSO",
        offset: 0,
        extension: ".dso",
    },
    // ELF: this system has a POSIX layer, so an ELF binary is one of its own,
    // and the table has held `.elf` as an executable since 2026-09-16. This
    // entry answered "unknown" until 2026-09-27, from before that decision.
    MagicSignature {
        bytes: b"\x7fELF",
        offset: 0,
        extension: ".elf",
    },
    // Images
    MagicSignature {
        bytes: b"\x89PNG\r\n\x1a\n",
        offset: 0,
        extension: ".png",
    },
    MagicSignature {
        bytes: b"\xff\xd8\xff",
        offset: 0,
        extension: ".jpg",
    },
    MagicSignature {
        bytes: b"GIF89a",
        offset: 0,
        extension: ".gif",
    },
    MagicSignature {
        bytes: b"GIF87a",
        offset: 0,
        extension: ".gif",
    },
    MagicSignature {
        bytes: b"BM",
        offset: 0,
        extension: ".bmp",
    },
    // TIFF, in either byte order.
    MagicSignature {
        bytes: b"II*\x00",
        offset: 0,
        extension: ".tiff",
    },
    MagicSignature {
        bytes: b"MM\x00*",
        offset: 0,
        extension: ".tiff",
    },
    // RIFF containers, told apart by the form type at offset 8. A bare `RIFF`
    // at 0 stood here until 2026-09-27, ahead of `WEBP` and mapped to WAV, so
    // every WebP and every AVI file was reported as WAV audio.
    MagicSignature {
        bytes: b"WAVE",
        offset: 8,
        extension: ".wav",
    },
    MagicSignature {
        bytes: b"WEBP",
        offset: 8,
        extension: ".webp",
    },
    MagicSignature {
        bytes: b"AVI ",
        offset: 8,
        extension: ".avi",
    },
    // Documents
    MagicSignature {
        bytes: b"%PDF",
        offset: 0,
        extension: ".pdf",
    },
    // Archives. A package (`.pkg`) is a ZIP too; the caller tells them apart by
    // extension.
    MagicSignature {
        bytes: b"PK\x03\x04",
        offset: 0,
        extension: ".zip",
    },
    MagicSignature {
        bytes: b"\x1f\x8b",
        offset: 0,
        extension: ".tar.gz",
    },
    MagicSignature {
        bytes: b"BZh",
        offset: 0,
        extension: ".tar.bz2",
    },
    MagicSignature {
        bytes: b"\xfd7zXZ\x00",
        offset: 0,
        extension: ".tar.xz",
    },
    MagicSignature {
        bytes: b"7z\xbc\xaf\x27\x1c",
        offset: 0,
        extension: ".7z",
    },
    MagicSignature {
        bytes: b"Rar!\x1a\x07",
        offset: 0,
        extension: ".rar",
    },
    MagicSignature {
        bytes: b"\x28\xb5\x2f\xfd",
        offset: 0,
        extension: ".zst",
    },
    MagicSignature {
        bytes: b"\x04\x22\x4d\x18",
        offset: 0,
        extension: ".lz4",
    },
    // POSIX tar: `ustar` in the header block, after the 257 bytes of name,
    // mode and sizes.
    MagicSignature {
        bytes: b"ustar",
        offset: 257,
        extension: ".tar",
    },
    // cpio, in its three ASCII header forms (new, new with checksum, old).
    MagicSignature {
        bytes: b"070701",
        offset: 0,
        extension: ".cpio",
    },
    MagicSignature {
        bytes: b"070702",
        offset: 0,
        extension: ".cpio",
    },
    MagicSignature {
        bytes: b"070707",
        offset: 0,
        extension: ".cpio",
    },
    // An `ar` archive: a static library.
    MagicSignature {
        bytes: b"!<arch>\n",
        offset: 0,
        extension: ".a",
    },
    // Audio / Video
    MagicSignature {
        bytes: b"fLaC",
        offset: 0,
        extension: ".flac",
    },
    MagicSignature {
        bytes: b"OggS",
        offset: 0,
        extension: ".ogg",
    },
    MagicSignature {
        bytes: b"ID3",
        offset: 0,
        extension: ".mp3",
    },
    MagicSignature {
        bytes: b"\xff\xfb",
        offset: 0,
        extension: ".mp3",
    },
    MagicSignature {
        bytes: b"MThd",
        offset: 0,
        extension: ".mid",
    },
    // ISO: the "CD001" identifier at offset 0x8001 (sector 16).
    MagicSignature {
        bytes: b"CD001",
        offset: 0x8001,
        extension: ".iso",
    },
    // ISO Base Media pictures, told apart from video by the `ftyp` box's
    // major brand, which follows the box type. Above the generic `ftyp`
    // entry because the table answers with its first match: without these,
    // every AVIF and HEIC photo was reported as an MP4 video. A sequence
    // (`avis`) is still an AVIF file -- an animated picture, not a video.
    //
    // Only the major brand is read. A file whose major brand is the generic
    // `mif1` but which lists `avif` among its compatible brands is reported
    // as HEIF here -- still a picture, which is the error that matters;
    // `imagecodec::avif::is_avif` reads the whole box and decodes it anyway.
    MagicSignature {
        bytes: b"ftypavif",
        offset: 4,
        extension: ".avif",
    },
    MagicSignature {
        bytes: b"ftypavis",
        offset: 4,
        extension: ".avif",
    },
    MagicSignature {
        bytes: b"ftypheic",
        offset: 4,
        extension: ".heic",
    },
    MagicSignature {
        bytes: b"ftypheix",
        offset: 4,
        extension: ".heic",
    },
    MagicSignature {
        bytes: b"ftypmif1",
        offset: 4,
        extension: ".heif",
    },
    // ISO Base Media (MP4, MOV, M4A): an `ftyp` box.
    MagicSignature {
        bytes: b"ftyp",
        offset: 4,
        extension: ".mp4",
    },
    // Matroska, and WebM, which is Matroska with another document type -- a
    // fixed-offset pattern cannot tell the two apart, so both read as `.mkv`.
    MagicSignature {
        bytes: b"\x1a\x45\xdf\xa3",
        offset: 0,
        extension: ".mkv",
    },
];

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
        // `get` returns `None` for a window that runs off the end, so "the
        // header is long enough" and "the bytes match" are one question here
        // rather than a length test standing above the slice it licenses.
        if header.get(sig.offset..end) != Some(sig.bytes) {
            continue;
        }
        let info = detect_from_extension(sig.extension);
        // A signature naming no row would answer with the "unknown" sentinel,
        // which a caller could not tell from "no match".
        // `every_signature_names_a_row_of_the_table` makes this unreachable;
        // it stays so a mistake reads as "no match", not as a wrong type.
        return (info.category != FileCategory::Unknown).then_some(info);
    }
    None
}

/// Return the [`FileCategory`] for a given extension string.
pub fn category_from_extension(ext: &str) -> FileCategory {
    detect_from_extension(ext).category
}

/// Every file type this table knows.
///
/// The table itself stays private: a caller that could index it could also
/// come to depend on its order, and the order here is grouping-by-hand rather
/// than a guarantee. An iterator gives what a caller legitimately needs --
/// `apps/fileassoc` builds its registry of known types from this, instead of
/// the second hand-written table of extensions, MIME types and descriptions it
/// used to carry.
pub fn all() -> impl Iterator<Item = &'static FileTypeInfo> {
    FILE_TYPE_TABLE.iter()
}

/// Every extension this table files under `category`, without leading dots.
///
/// The reverse of [`category_from_extension`], from the same table, which is
/// the point: a caller that needs "all the audio extensions" would otherwise
/// write its own list, and a second hand-written list of the same facts drifts
/// from this one silently -- nothing would fail to compile when a format was
/// added here and not there.
///
/// Used by `gui/associations` to turn "set the music player" into the set of
/// extensions to write. See design-decisions 857.
pub fn extensions_in(category: FileCategory) -> impl Iterator<Item = &'static str> {
    FILE_TYPE_TABLE
        .iter()
        .filter(move |info| info.category == category)
        .map(|info| info.extension.strip_prefix('.').unwrap_or(info.extension))
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    // -- Extension detection by category -----------------------------------

    #[test]
    fn detect_os_specific_extensions() {
        assert_eq!(
            detect_from_extension(".nx").category,
            FileCategory::Executable
        );
        assert_eq!(
            detect_from_extension(".dso").category,
            FileCategory::Library
        );
        assert_eq!(
            detect_from_extension(".slib").category,
            FileCategory::Library
        );
        assert_eq!(
            detect_from_extension(".pkg").category,
            FileCategory::Package
        );
    }

    #[test]
    fn detect_document_extensions() {
        assert_eq!(
            detect_from_extension(".txt").category,
            FileCategory::Document
        );
        assert_eq!(
            detect_from_extension(".md").category,
            FileCategory::Document
        );
        assert_eq!(
            detect_from_extension(".pdf").category,
            FileCategory::Document
        );
        assert_eq!(
            detect_from_extension(".html").category,
            FileCategory::Document
        );
        assert_eq!(
            detect_from_extension(".docx").category,
            FileCategory::Document
        );
    }

    #[test]
    fn detect_image_extensions() {
        for ext in &[
            ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".svg", ".ico", ".webp", ".tiff", ".avif",
            ".heic", ".heif",
        ] {
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
        assert_eq!(
            detect_from_extension(".iso").category,
            FileCategory::DiskImage
        );
        assert_eq!(
            detect_from_extension(".img").category,
            FileCategory::DiskImage
        );
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

    /// **A picture in an ISO Base Media box is a picture.** AVIF and HEIC open
    /// with the same `ftyp` box as an MP4, and the brand after it is what
    /// says which: every one of these was reported as MP4 video, so a photo
    /// went to the video player.
    #[test]
    fn an_iso_media_picture_is_not_read_as_video() {
        let ftyp = |brand: &[u8; 4]| {
            let mut header = b"\x00\x00\x00\x1cftyp".to_vec();
            header.extend_from_slice(brand);
            header.extend_from_slice(b"\x00\x00\x00\x00mif1miaf");
            detect_from_magic(&header).map(|info| (info.extension, info.mime_type, info.category))
        };
        let picture = |ext, mime| Some((ext, mime, FileCategory::Image));
        assert_eq!(ftyp(b"avif"), picture(".avif", "image/avif"));
        assert_eq!(
            ftyp(b"avis"),
            picture(".avif", "image/avif"),
            "an animated AVIF is a picture"
        );
        assert_eq!(ftyp(b"heic"), picture(".heic", "image/heic"));
        assert_eq!(ftyp(b"heix"), picture(".heic", "image/heic"));
        assert_eq!(ftyp(b"mif1"), picture(".heif", "image/heif"));
        // The control: video is still video, whatever its brand.
        for brand in [b"isom", b"mp42", b"M4V ", b"qt  "] {
            assert_eq!(
                ftyp(brand).map(|(ext, _, category)| (ext, category)),
                Some((".mp4", FileCategory::Video)),
                "{brand:?}"
            );
        }
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

    /// An ELF binary is one of this system's own (it has a POSIX layer), and
    /// the table has held `.elf` since 2026-09-16; the signature said
    /// "unknown" until 2026-09-27, from before that decision.
    #[test]
    fn magic_detects_elf() {
        let header = b"\x7fELF\x02\x01\x01\x00";
        let info = detect_from_magic(header).expect("ELF");
        assert_eq!(info.extension, ".elf");
        assert_eq!(info.category, FileCategory::Executable);
    }

    /// Every signature names a row of the table, so a detection can never
    /// answer with the "unknown" sentinel or with a row that does not exist.
    #[test]
    fn every_signature_names_a_row_of_the_table() {
        for sig in MAGIC_TABLE {
            assert!(
                all().any(|info| info.extension == sig.extension),
                "a signature names {:?}, which the table does not hold",
                sig.extension
            );
        }
    }

    /// A RIFF file is told apart by its form type. A bare `RIFF` signature
    /// stood ahead of `WEBP` and answered WAV, so every WebP picture and every
    /// AVI video was reported as audio.
    #[test]
    fn a_riff_file_is_what_its_form_type_says() {
        let riff = |form: &[u8; 4]| {
            let mut header = b"RIFF\x24\x00\x00\x00".to_vec();
            header.extend_from_slice(form);
            header.extend_from_slice(b"\x00\x00\x00\x00");
            detect_from_magic(&header).map(|info| info.extension)
        };
        assert_eq!(riff(b"WAVE"), Some(".wav"));
        assert_eq!(riff(b"WEBP"), Some(".webp"));
        assert_eq!(riff(b"AVI "), Some(".avi"));
        assert_eq!(riff(b"XXXX"), None, "an unknown RIFF form is not WAV");
    }

    /// The signatures carried from the kernel's `fs::mime` when the program
    /// lists became one (gui/programs/INVENTORY.md section 5).
    #[test]
    fn the_kernels_signatures_are_recognised() {
        let mut tar = vec![0u8; 512];
        tar[257..262].copy_from_slice(b"ustar");
        let cases: [(&[u8], &str); 11] = [
            (b"II*\x00\x08\x00\x00\x00", ".tiff"),
            (b"MM\x00*\x00\x00\x00\x08", ".tiff"),
            (b"\x28\xb5\x2f\xfd\x00\x00", ".zst"),
            (b"\x04\x22\x4d\x18\x00\x00", ".lz4"),
            (&tar, ".tar"),
            (b"070701000000", ".cpio"),
            (b"070702000000", ".cpio"),
            (b"070707000000", ".cpio"),
            (b"!<arch>\nfile.o", ".a"),
            (b"MThd\x00\x00\x00\x06", ".mid"),
            (b"\x7fELF\x02\x01", ".elf"),
        ];
        for (header, want) in cases {
            assert_eq!(
                detect_from_magic(header).map(|info| info.extension),
                Some(want),
                "{header:?}"
            );
        }
    }

    /// The extensions carried from the kernel's `fs::mime` and `fs::filetype`,
    /// each with the kernel's type (gui/programs/INVENTORY.md section 5).
    #[test]
    fn the_kernels_extensions_are_recognised() {
        for (ext, mime, category) in [
            ("bat", "text/x-msdos-batch", FileCategory::Code),
            ("cmd", "text/x-msdos-batch", FileCategory::Code),
            ("cc", "text/x-c++", FileCategory::Code),
            ("cxx", "text/x-c++", FileCategory::Code),
            ("hxx", "text/x-c++", FileCategory::Code),
            ("mjs", "text/javascript", FileCategory::Code),
            ("psm1", "application/x-powershell", FileCategory::Code),
            ("pyw", "text/x-python", FileCategory::Code),
            ("diff", "text/x-diff", FileCategory::Code),
            ("patch", "text/x-diff", FileCategory::Code),
            ("htm", "text/html", FileCategory::Document),
            ("markdown", "text/markdown", FileCategory::Document),
            ("text", "text/plain", FileCategory::Document),
            ("xsd", "application/xml", FileCategory::Data),
            ("xsl", "application/xml", FileCategory::Data),
            ("cpio", "application/x-cpio", FileCategory::Archive),
            ("gzip", "application/gzip", FileCategory::Archive),
            ("zstd", "application/zstd", FileCategory::Archive),
            ("jar", "application/java-archive", FileCategory::Archive),
            ("a", "application/x-archive", FileCategory::Library),
            ("lib", "application/x-archive", FileCategory::Library),
            ("o", "application/x-object", FileCategory::Library),
            ("epub", "application/epub+zip", FileCategory::Document),
        ] {
            let info = detect_from_extension(ext);
            assert_eq!(info.mime_type, mime, ".{ext}");
            assert_eq!(info.category, category, ".{ext}");
        }
    }

    /// The kernel knew nine more, and each is waiting on a decision this table
    /// has recorded rather than on effort (`known-issues.md`, the `filesearch`
    /// entry): foreign executables and installers -- this system cannot run
    /// them -- and databases, which need a kind the category enum does not
    /// have. Pinned so the next inventory does not "finish the job".
    #[test]
    fn the_kernels_types_this_table_waits_to_decide_are_still_absent() {
        for ext in [
            "exe", "dll", "class", "wasm", "deb", "rpm", "db", "sqlite", "sqlite3",
        ] {
            assert_eq!(
                category_from_extension(ext),
                FileCategory::Unknown,
                ".{ext} was added without the decision that gates it"
            );
        }
        for header in [
            &b"MZ\x90\x00"[..],
            b"SQLite format 3\x00",
            b"\x00asm\x01\x00",
        ] {
            assert!(detect_from_magic(header).is_none(), "{header:?}");
        }
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
        assert_eq!(
            mime_for_extension(".nx"),
            "application/x-slateos-executable"
        );
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
        assert_eq!(
            detect_from_extension(".NX").category,
            FileCategory::Executable
        );
        assert_eq!(
            detect_from_extension(".TAR.GZ").category,
            FileCategory::Archive
        );
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

    // -- FileTypeInfo fields ------------------------------------------------

    #[test]
    fn file_type_info_fields() {
        let info = detect_from_extension(".nx");
        assert_eq!(info.description, "Slate OS Native Executable");
        assert!(info.is_executable);
        assert!(!info.is_text);

        let info = detect_from_extension(".rs");
        assert_eq!(info.description, "Rust Source File");
        assert!(info.is_text);
    }

    // -- Compound extensions -----------------------------------------------

    #[test]
    fn compound_extensions() {
        assert_eq!(
            detect_from_extension(".tar.gz").category,
            FileCategory::Archive
        );
        assert_eq!(
            detect_from_extension(".tar.bz2").category,
            FileCategory::Archive
        );
        assert_eq!(
            detect_from_extension(".tar.xz").category,
            FileCategory::Archive
        );
        assert_eq!(
            detect_from_extension(".tgz").category,
            FileCategory::Archive
        );
    }

    // -- DIR_ICON_GLYPH constant -------------------------------------------

    #[test]
    fn dir_icon_is_folder() {
        assert_eq!(DIR_ICON_GLYPH, '\u{1F4C1}');
    }

    /// The two directions agree, for every extension in the table.
    ///
    /// This is the property that makes `extensions_in` safe to build a
    /// settings page on: if it ever returned an extension that
    /// `category_from_extension` files elsewhere, choosing a music player
    /// would write an association for something that is not audio.
    #[test]
    fn every_listed_extension_maps_back_to_its_own_category() {
        for category in [
            FileCategory::Audio,
            FileCategory::Video,
            FileCategory::Image,
            FileCategory::Document,
            FileCategory::Archive,
            FileCategory::Code,
        ] {
            for extension in extensions_in(category) {
                assert_eq!(
                    category_from_extension(extension),
                    category,
                    ".{extension} is listed under {category:?} but resolves elsewhere"
                );
            }
        }
    }

    /// The categories a default-application row is offered for are not empty.
    ///
    /// A category with no extensions would make a row that wrote nothing --
    /// the silent no-op design-decisions 856 is about.
    #[test]
    fn the_offered_categories_have_extensions() {
        for category in [
            FileCategory::Audio,
            FileCategory::Video,
            FileCategory::Image,
        ] {
            assert!(
                extensions_in(category).count() > 1,
                "{category:?} covers {} extension(s)",
                extensions_in(category).count()
            );
        }
    }

    /// Documents genuinely span programs, which is why there is no such row.
    ///
    /// 857 drops the Documents category on the claim that its members do not
    /// share a program -- a .txt is edited and a .pdf is viewed. That claim is
    /// about *this* table, so it is checked here rather than asserted in prose:
    /// if the table ever stopped filing both under Document, the decision
    /// would need revisiting and this test is what would say so.
    #[test]
    fn the_document_category_spans_kinds_that_do_not_share_a_program() {
        let documents: Vec<&str> = extensions_in(FileCategory::Document).collect();
        assert!(
            documents.contains(&"txt") && documents.contains(&"pdf"),
            "Document holds {documents:?}"
        );
    }

    /// The formats added on 2026-09-16 classify as intended.
    ///
    /// Named one at a time rather than counted: a count would pass if an entry
    /// were added under the wrong category, which is the mistake a bulk edit
    /// actually makes.
    #[test]
    fn the_formats_added_from_filesearch_resolve() {
        for (ext, want) in [
            ("mpg", FileCategory::Video),
            ("mpeg", FileCategory::Video),
            ("m4v", FileCategory::Video),
            ("vob", FileCategory::Video),
            ("psd", FileCategory::Image),
            ("yml", FileCategory::Config),
            ("tex", FileCategory::Document),
            ("bash", FileCategory::Code),
            ("zsh", FileCategory::Code),
            ("fish", FileCategory::Code),
            ("ps1", FileCategory::Code),
            ("properties", FileCategory::Config),
            ("cab", FileCategory::Archive),
            ("lz4", FileCategory::Archive),
            ("elf", FileCategory::Executable),
            ("so", FileCategory::Library),
        ] {
            assert_eq!(
                category_from_extension(ext),
                want,
                ".{ext} does not classify as {want:?}"
            );
        }
    }

    /// Foreign executables stay out until somebody decides they belong.
    ///
    /// Pinned because the next person diffing this table against
    /// `apps/filesearch` will find them missing and be tempted to "finish the
    /// job". Listing them would have the table claim a kind for a file nothing
    /// on this system can open, and that is a decision, not an omission. If it
    /// is ever made, delete this test in the same change.
    #[test]
    fn foreign_executables_are_absent_on_purpose() {
        for ext in ["exe", "dll", "msi", "app", "dylib"] {
            assert_eq!(
                category_from_extension(ext),
                FileCategory::Unknown,
                ".{ext} was added without the decision that gates it"
            );
        }
    }

    /// No two entries claim the same extension.
    ///
    /// `detect_from_extension` returns the first match, so a duplicate makes
    /// the second entry unreachable and every count over the table wrong by
    /// one, with nothing failing anywhere.
    #[test]
    fn no_extension_appears_twice_in_the_table() {
        let mut seen: Vec<&str> = Vec::new();
        for info in all() {
            let ext = info.extension.strip_prefix('.').unwrap_or(info.extension);
            assert!(
                !seen.contains(&ext),
                "the table lists .{ext} more than once"
            );
            seen.push(ext);
        }
    }
}
