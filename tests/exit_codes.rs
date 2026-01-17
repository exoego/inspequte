use std::process::Command;

#[test]
fn inspequte_exits_non_zero_on_error() {
    let inspequte = std::env::var("CARGO_BIN_EXE_inspequte")
        .or_else(|_| std::env::var("CARGO_BIN_EXE_inspequte"))
        .unwrap_or_else(|_| {
            let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("target");
            path.push("debug");
            path.push("inspequte");
            if cfg!(windows) {
                path.set_extension("exe");
            }
            path.to_string_lossy().to_string()
        });
    let output = Command::new(inspequte)
        .arg("--input")
        .arg("missing.class")
        .output()
        .expect("run inspequte");

    assert!(!output.status.success());
}
