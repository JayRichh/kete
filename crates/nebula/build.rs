//! Embeds the app icon and version info in `kete.exe` (and its `bro.exe`
//! alias) on Windows. Other platforms carry their icon in the installer,
//! not the binary.

fn main() {
    // Windows gives a program's main thread a 1 MiB stack, and clap's
    // derived parser for the whole command tree builds in a handful of
    // very large frames in an unoptimised build: 0.60's studio, roles and
    // usage commands took a debug `kete --version` past it. 8 MiB is what
    // Linux and macOS give the main thread anyway; only the reservation
    // grows, not what the process uses.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:8388608"),
            Ok("gnu") => println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608"),
            _ => {}
        }
    }
    #[cfg(windows)]
    {
        const ICON: &str = "../../assets/brand/kete/kete.ico";
        println!("cargo:rerun-if-changed={ICON}");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ICON)
            .set("ProductName", "kete")
            .set("FileDescription", "kete: sweet as agent management");
        if let Err(e) = res.compile() {
            println!("cargo:warning=app icon not embedded: {e}");
        }
    }
}
