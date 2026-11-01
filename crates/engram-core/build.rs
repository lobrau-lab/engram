use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native/mlx_bridge.swift");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let lib_path = out_dir.join("libengram_mlx.a");
    let source = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("native")
        .join("mlx_bridge.swift");

    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-O",
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-module-name",
            "EngramMlxBridge",
            source.to_str().expect("valid UTF-8 path"),
            "-o",
            lib_path.to_str().expect("valid UTF-8 output path"),
        ])
        .status()
        .expect("failed to invoke xcrun swiftc for mlx_bridge.swift");
    assert!(status.success(), "xcrun swiftc failed to compile mlx_bridge.swift");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=engram_mlx");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=Accelerate");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
