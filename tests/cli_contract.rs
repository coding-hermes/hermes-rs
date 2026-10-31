use std::process::Command;

#[test]
fn cli_contract_covers_scaffold_help_and_unknown_arguments() {
    let no_args = Command::new(env!("CARGO_BIN_EXE_hermes-rs"))
        .output()
        .expect("run built hermes-rs executable without arguments");
    assert!(
        no_args.status.success(),
        "process status: {}",
        no_args.status
    );
    assert!(
        String::from_utf8_lossy(&no_args.stdout).contains("scaffold"),
        "stdout should report scaffold status: {:?}",
        no_args.stdout
    );

    let help = Command::new(env!("CARGO_BIN_EXE_hermes-rs"))
        .arg("--help")
        .output()
        .expect("run built hermes-rs executable with --help");
    assert!(help.status.success(), "process status: {}", help.status);
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("Usage"), "help output: {help_stdout}");
    assert!(
        help_stdout.contains("hermes-rs"),
        "help output: {help_stdout}"
    );

    let unknown = Command::new(env!("CARGO_BIN_EXE_hermes-rs"))
        .arg("--bogus")
        .output()
        .expect("run built hermes-rs executable with an unknown argument");
    assert!(
        !unknown.status.success(),
        "process status: {}",
        unknown.status
    );
    let unknown_stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(
        unknown_stderr.contains("unexpected argument"),
        "unknown-argument diagnostic should be actionable: {unknown_stderr}"
    );
}
