extern crate napi_build;

fn main() {
    napi_build::setup();

    // Version injection: prefer EVOT_VERSION env var (set by CI), fallback to CARGO_PKG_VERSION.
    if let Ok(version) = std::env::var("EVOT_VERSION") {
        println!("cargo:rustc-env=EVOT_VERSION={version}");
    } else {
        println!(
            "cargo:rustc-env=EVOT_VERSION={}",
            std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string())
        );
    }
}
