use std::env;
use std::fs;
use std::path::PathBuf;

/// WCH's closed-source libraries, as `mise run fw:wch` downloads them: the
/// BLE peripheral stack and the flash/ROM access library (only its info
/// area read is used, for the chip's MAC address).
const WCH_LIBS: [&str; 2] = ["CH572BLE_PERI", "ISP572"];

fn main() {
    // The linker script and WCH's libraries only apply to the firmware
    // image.  Host test builds of the library link normally.
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("riscv32") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("link.x"), include_bytes!("link.x")).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=link.x");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");

    let dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("wch");
    for lib in WCH_LIBS {
        let file = dir.join(format!("lib{lib}.a"));
        if !file.exists() {
            panic!(
                "{} is missing: run `mise run fw:wch` to download WCH's libraries",
                file.display()
            );
        }
        println!("cargo:rerun-if-changed={}", file.display());
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-search={}", dir.display());
}
