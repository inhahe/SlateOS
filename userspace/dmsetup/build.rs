fn main() {
    // Embed a Windows application manifest that declares "asInvoker" execution
    // level. Without this, Windows heuristics detect "setup" in the binary
    // name and demand elevation (UAC), which breaks `cargo test`.
    #[cfg(target_os = "windows")]
    {
        static MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#;

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let manifest_path = std::path::Path::new(&out_dir).join("dmsetup.exe.manifest");
        std::fs::write(&manifest_path, MANIFEST).unwrap();

        // For MSVC targets, embed the manifest via the linker.
        let target = std::env::var("TARGET").unwrap_or_default();
        if target.contains("msvc") {
            println!(
                "cargo:rustc-link-arg-bins=/MANIFEST:EMBED",
            );
            println!(
                "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
                manifest_path.display()
            );
        } else if target.contains("gnu") {
            // For GNU targets on Windows, use windres via a .rc file.
            let rc_path = std::path::Path::new(&out_dir).join("manifest.rc");
            std::fs::write(
                &rc_path,
                format!("1 24 \"{}\"", manifest_path.display().to_string().replace('\\', "\\\\")),
            )
            .unwrap();
            // Try to compile the resource file.
            let status = std::process::Command::new("windres")
                .arg(&rc_path)
                .arg("-o")
                .arg(std::path::Path::new(&out_dir).join("manifest.o"))
                .arg("--output-format=coff")
                .status();
            if let Ok(s) = status {
                if s.success() {
                    println!("cargo:rustc-link-arg-bins={}", std::path::Path::new(&out_dir).join("manifest.o").display());
                }
            }
        }
    }
}
