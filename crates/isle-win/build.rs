use std::{env, path::PathBuf, process::Command};

// Embeds the Buginotch icons into the exe (Windows targets only).
fn main() {
    println!("cargo:rerun-if-changed=assets/buginotch.rc");
    println!("cargo:rerun-if-changed=assets/buginotch.ico");
    println!("cargo:rerun-if-changed=assets/buginotch-tray.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let obj = out.join("buginotch-res.o");
    let tools = ["x86_64-w64-mingw32-windres", "windres"];
    for t in tools {
        let ok = Command::new(t)
            .current_dir("assets")
            .args(["-i", "buginotch.rc", "-o"])
            .arg(&obj)
            .args(["-O", "coff"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bins={}", obj.display());
            return;
        }
    }
    println!("cargo:warning=windres not found; building without the app icon");
}
