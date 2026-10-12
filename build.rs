fn main() {
    if std::env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        // linux
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
            println!("cargo:rustc-link-lib=dylib:+verbatim=libSDL2-2.0.so.0");
        } else {
            println!("cargo:rustc-link-lib=SDL2");
        }
    }
}
