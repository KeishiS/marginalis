use std::process::Command;

#[test]
fn version_flags_report_the_packaged_version() {
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
            .arg(flag)
            .output()
            .expect("run marginalis");

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 stdout"),
            format!("marginalis {}\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(output.stderr.is_empty());
    }
}
