//! Build script. Its only job: give the Windows `.exe` its icon and version info.
//!
//! The window icon at runtime comes from `WindowProps::icon`; this is the icon
//! Explorer, the taskbar pin and the Start menu show for the file itself, which
//! Windows reads from a resource compiled into the executable.
//!
//! `cfg(windows)` here is the *host*, matching the host-only build-dependency in
//! Cargo.toml: Windows builds are made on Windows (the release workflow's
//! `windows-latest` runner), so Linux and the trunk/wasm build never compile
//! `winresource` at all.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=packaging/windows/plotweb.ico");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/plotweb.ico")
            .set("ProductName", "PlotWeb")
            .set("FileDescription", "PlotWeb")
            .set("InternalName", "plotweb")
            .set("OriginalFilename", "PlotWeb.exe");
        res.compile().expect("compile Windows resources");
    }
}
