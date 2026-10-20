fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // chat-applefm's Swift bridge references the Swift concurrency
        // runtime through @rpath; point it at the OS runtime.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
