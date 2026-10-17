#[cfg(target_os = "macos")]
#[test]
fn floating_microphone_is_a_real_nonactivating_panel() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_speak"))
        .arg("--check-floating-panel")
        .output()
        .expect("Could not run native panel regression check");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("non-activating"));
}
