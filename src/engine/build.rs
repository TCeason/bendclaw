fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=EVOT_VERSION");

    let version = std::env::var("EVOT_VERSION")
        .or_else(|_| std::env::var("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| "0.1.0".into());

    let out_dir = std::env::var("OUT_DIR")?;
    let dest = std::path::Path::new(&out_dir).join("user_agent.rs");
    std::fs::write(
        &dest,
        format!("const USER_AGENT: &str = \"evot/{version}\";\n"),
    )?;

    Ok(())
}
