use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native/control.swift");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let helper = out_dir.join("engram-control");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        let source = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"))
            .join("native")
            .join("control.swift");
        let built = Command::new("xcrun")
            .args([
                "swiftc",
                "-O",
                source
                    .to_str()
                    .expect("control.swift path must be valid UTF-8"),
                "-o",
                helper.to_str().expect("helper path must be valid UTF-8"),
            ])
            .status();
        match built {
            Ok(status) if status.success() => {}
            Ok(status) => {
                println!("cargo:warning=GUI takeover helper did not compile ({status}); building a disabled fallback");
                write_fallback(&helper, "The GUI takeover helper did not compile. Install Xcode Command Line Tools and rebuild Engram.");
            }
            Err(error) => {
                println!("cargo:warning=could not run xcrun swiftc ({error}); building a disabled fallback");
                write_fallback(&helper, "The GUI takeover helper was not built. Install Xcode Command Line Tools and rebuild Engram.");
            }
        }
    } else {
        write_fallback(&helper, "GUI takeover is only available on macOS.");
    }

    println!("cargo:rustc-env=ENGRAM_CONTROL_BIN={}", helper.display());
}

fn write_fallback(path: &Path, message: &str) {
    let escaped = message
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    let json = format!("{{\"error\":\"{escaped}\"}}");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' {}\nexit 1\n",
        shell_single_quote(&json)
    );
    fs::write(path, script).expect("write helper fallback");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("mark helper fallback executable");
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
