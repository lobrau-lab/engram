use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native/control.swift");
    println!("cargo:rerun-if-changed=native/webview.swift");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let helper = out_dir.join("engram-control");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
        let source = manifest_dir.join("native").join("control.swift");
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

        let webview_source = manifest_dir.join("native").join("webview.swift");
        let webview_lib = out_dir.join("libengram_webview.a");
        let webview_status = Command::new("xcrun")
            .args([
                "swiftc",
                "-O",
                "-parse-as-library",
                "-emit-library",
                "-static",
                "-module-name",
                "EngramWebView",
                webview_source.to_str().expect("valid UTF-8 path"),
                "-o",
                webview_lib.to_str().expect("valid UTF-8 output path"),
            ])
            .status()
            .expect("failed to invoke xcrun swiftc for webview.swift");
        assert!(
            webview_status.success(),
            "xcrun swiftc failed to compile webview.swift"
        );
        println!("cargo:rustc-link-search=native={}", out_dir.display());
        println!("cargo:rustc-link-lib=static=engram_webview");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=WebKit");
    } else {
        write_fallback(&helper, "GUI takeover is only available on macOS.");
    }

    println!("cargo:rustc-env=ENGRAM_CONTROL_BIN={}", helper.display());
    generate_manifest(
        &PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("web"),
        &out_dir,
    );
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

fn generate_manifest(web: &Path, out_dir: &Path) {
    println!("cargo:rerun-if-changed={}", web.display());

    let mut files = Vec::new();
    collect(&web, &web, &mut files);
    files.sort();

    let mut manifest = String::from(
        "struct ThreadlineAsset {\n    path: &'static str,\n    content_type: &'static str,\n    bytes: &'static [u8],\n}\n\nstatic THREADLINE_ASSETS: &[ThreadlineAsset] = &[\n",
    );
    for (route, path, content_type) in files {
        manifest.push_str(&format!(
            "    ThreadlineAsset {{ path: {route:?}, content_type: {content_type:?}, bytes: include_bytes!({:?}) }},\n",
            path.to_string_lossy()
        ));
    }
    manifest.push_str("];\n");

    let out = out_dir.join("threadline_assets.rs");
    fs::write(out, manifest).unwrap();
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, PathBuf, &'static str)>) {
    let mut entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("reading {}: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();

    for path in entries {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if name == ".DS_Store" || name == "README.md" || name == "tests" {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let relative = path.strip_prefix(root).unwrap();
        let content_type = match path.extension().and_then(|extension| extension.to_str()) {
            Some("html") => "text/html; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("js") => "text/javascript; charset=utf-8",
            Some("png") => "image/png",
            Some("svg") => "image/svg+xml",
            extension => panic!("unsupported Threadline asset type: {} ({extension:?})", path.display()),
        };
        files.push((
            format!("/threadline/{}", relative.to_string_lossy().replace('\\', "/")),
            path,
            content_type,
        ));
    }
}
