use std::process::Command;

#[test]
fn executable_prints_the_library_scaffold_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_hermes-rs"))
        .output()
        .expect("run built hermes-rs executable");

    assert!(output.status.success(), "process status: {}", output.status);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Hermes RS scaffold: no agent functionality implemented\n"
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
}
