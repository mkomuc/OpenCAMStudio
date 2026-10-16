use std::process::Command;

fn app() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opencamstudio"))
}

#[test]
fn help_succeeds_without_opening_the_gui() {
    let output = app().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--setup <path.ocam>"));
}

#[test]
fn bad_arguments_return_usage_error() {
    for args in [vec!["--setup"], vec!["--unknown"]] {
        let output = app().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
}

#[test]
fn startup_failure_returns_nonzero_and_explains_the_problem() {
    let path =
        std::env::temp_dir().join(format!("missing-startup-cli-{}.ocam", std::process::id()));
    let output = app().arg("--setup").arg(&path).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    #[cfg(feature = "gui")]
    assert!(stderr.contains("Could not load setup"), "{stderr}");
    #[cfg(not(feature = "gui"))]
    assert!(stderr.contains("requires the desktop app"), "{stderr}");
}
