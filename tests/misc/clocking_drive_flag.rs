//! `--clocking-drive=<lrm|next-edge>` and `XEZIM_CLOCKING_DRIVE`, end to end:
//! both spellings of the flag, the variable, the flag winning over it, and an
//! unknown value from either exiting 1. The fixture is
//! `tests/lrm/sv/c14_offedge_negedge.sv`, where `branch` rises at 10 ns only
//! under `next-edge`.
use std::process::Command;

const SRC: &str = include_str!("../lrm/sv/c14_offedge_negedge.sv");

fn run(tag: &str, args: &[&str], env: Option<&str>) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!(
        "xezim_clocking_drive_{}_{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("c14on.sv");
    std::fs::write(&path, SRC).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xezim"));
    cmd.args(args)
        .args(["--no-cache", "-s", "c14on"])
        .arg(&path);
    cmd.env_remove("XEZIM_CLOCKING_DRIVE");
    if let Some(v) = env {
        cmd.env("XEZIM_CLOCKING_DRIVE", v);
    }
    let out = cmd.output().expect("run xezim");
    let _ = std::fs::remove_dir_all(&dir);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

const ROSE: &str = "T|chg|t=10000 branch=1";

#[test]
fn clocking_drive_flag_and_env_pick_the_mode() {
    for (tag, args, env, rises) in [
        ("default", &[][..], None, false),
        ("lrm", &["--clocking-drive=lrm"][..], None, false),
        ("eq", &["--clocking-drive=next-edge"][..], None, true),
        ("space", &["--clocking-drive", "next-edge"][..], None, true),
        ("env", &[][..], Some("next-edge"), true),
        (
            "flag_wins",
            &["--clocking-drive=lrm"][..],
            Some("next-edge"),
            false,
        ),
    ] {
        let (code, text) = run(tag, args, env);
        assert_eq!(code, 0, "{tag}: {text}");
        assert!(
            text.contains("T|pos|t=35000 branch=0"),
            "{tag}: run did not finish: {text}"
        );
        assert_eq!(text.contains(ROSE), rises, "{tag}: {text}");
    }
}

#[test]
fn clocking_drive_unknown_value_exits_1() {
    let (code, text) = run("bad_flag", &["--clocking-drive=next_edge"], None);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("Error: --clocking-drive: unknown clocking drive mode 'next_edge'"),
        "{text}"
    );
    let (code, text) = run("bad_env", &[], Some("edge"));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("Error: XEZIM_CLOCKING_DRIVE: unknown clocking drive mode 'edge'"),
        "{text}"
    );
}
