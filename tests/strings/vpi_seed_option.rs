//! `-sv_seed <n|random>` (also `-svseed`, `-seed`) is a simulator option, not
//! a plusarg. It seeds the RNG, but `$test$plusargs`/`$value$plusargs`
//! (IEEE 1800-2017 §21.6) don't match it and the argv `vpi_get_vlog_info`
//! returns (§38.17) carries no `+seed=` for it, as on the reference simulator.
//! xezim used to turn it into a `+seed=<value>` plusarg, so a cocotb test run
//! with `-sv_seed random` died at start-up parsing `+seed=random` as its own
//! seed. A `+seed=` the user writes is still a plusarg everywhere, and still
//! seeds xezim's RNG.

use std::path::{Path, PathBuf};
use std::process::Command;

const TB: &str = r#"
module tb;
  string s;
  initial begin
    $display("SV_TEST|%0d", $test$plusargs("seed"));
    if ($value$plusargs("seed=%s", s)) $display("SV_VALUE|%s", s);
    else $display("SV_VALUE|none");
    $display("RAND|%0d %0d %0d", $urandom, $urandom, $urandom);
  end
endmodule
"#;

/// A scratch directory holding `tb.sv` and the VPI module that prints the
/// plusargs in `vpi_get_vlog_info`'s argv (tests/dpi/vpi_seed_argv.c).
fn setup(tag: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("xezim_vpi_seed_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.sv"), TB).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let so = dir.join("vpi_seed_argv.so");
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-I"])
        .arg(root.join("include"))
        .arg(root.join("tests/dpi/vpi_seed_argv.c"))
        .arg("-o")
        .arg(&so)
        .status()
        .expect("failed to launch cc");
    assert!(status.success(), "cc failed for vpi_seed_argv.c");
    (dir, so)
}

fn xezim(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("failed to run xezim");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Run `design` (default `tb.sv`) under the VPI module with `extra`; return
/// (stdout, stderr).
fn run(dir: &Path, so: &Path, design: &str, extra: &[&str]) -> (String, String) {
    let so = so.to_string_lossy();
    let mut args = vec!["--vpi-lib", &so, design];
    args.extend_from_slice(extra);
    let (code, out, err) = xezim(dir, &args);
    assert_eq!(code, 0, "xezim {:?} failed:\n{}{}", extra, out, err);
    (out, err)
}

fn line<'a>(out: &'a str, tag: &str) -> &'a str {
    out.lines()
        .find(|l| l.starts_with(tag))
        .unwrap_or_else(|| panic!("no {} line:\n{}", tag, out))
}

fn vpi_plusargs(out: &str) -> Vec<&str> {
    assert!(
        out.contains("VPI_ARGC|"),
        "VPI module did not run:\n{}",
        out
    );
    out.lines()
        .filter_map(|l| l.strip_prefix("VPI_PLUSARG|"))
        .collect()
}

/// The seed a `random` run drew and printed for replay.
fn drawn_seed(err: &str) -> String {
    let rest = err
        .split("random seed: ")
        .nth(1)
        .unwrap_or_else(|| panic!("no random seed printed:\n{}", err));
    rest.split_whitespace().next().unwrap().to_string()
}

#[test]
fn seed_options_seed_without_a_plusarg() {
    let (d, so) = setup("option");
    let (native, _) = run(&d, &so, "tb.sv", &["+seed=123"]);
    std::fs::write(d.join("seed.f"), "-svseed 123\n").unwrap();
    for args in [
        &["-sv_seed", "123"][..],
        &["-svseed", "123"],
        &["-seed", "123"],
        &["-svseed=123"],
        &["-seed=123"],
        &["-f", "seed.f"],
    ] {
        let (out, _) = run(&d, &so, "tb.sv", args);
        assert!(vpi_plusargs(&out).is_empty(), "{:?}:\n{}", args, out);
        assert_eq!(line(&out, "SV_TEST|"), "SV_TEST|0", "{:?}", args);
        assert_eq!(line(&out, "SV_VALUE|"), "SV_VALUE|none", "{:?}", args);
        // The RNG is seeded exactly as by +seed=123.
        assert_eq!(line(&out, "RAND|"), line(&native, "RAND|"), "{:?}", args);
    }
    // `random` draws a seed and prints it; the printed option replays the run.
    let (out, err) = run(&d, &so, "tb.sv", &["-sv_seed", "random"]);
    assert!(vpi_plusargs(&out).is_empty(), "{}", out);
    assert_eq!(line(&out, "SV_TEST|"), "SV_TEST|0");
    let seed = drawn_seed(&err);
    assert!(
        err.contains(&format!("replay with -sv_seed {}", seed)),
        "{}",
        err
    );
    let (replay, _) = run(&d, &so, "tb.sv", &["-sv_seed", &seed]);
    assert_eq!(line(&replay, "RAND|"), line(&out, "RAND|"));
    // Every spelling checks its value.
    for args in [&["-svseed", "abc"][..], &["-seed=-1"]] {
        let (code, _, err) = xezim(&d, &[&["tb.sv"][..], args].concat());
        assert_eq!(code, 1, "{:?}: {}", args, err);
        assert!(err.contains("non-negative integer or 'random'"), "{}", err);
    }
}

#[test]
fn seed_plusarg_stays_a_plusarg() {
    let (d, so) = setup("plusarg");
    let (out, _) = run(&d, &so, "tb.sv", &["+seed=123"]);
    assert_eq!(vpi_plusargs(&out), ["+seed=123"]);
    assert_eq!(line(&out, "SV_TEST|"), "SV_TEST|1");
    assert_eq!(line(&out, "SV_VALUE|"), "SV_VALUE|123");
    let (other, _) = run(&d, &so, "tb.sv", &["+seed=124"]);
    assert_ne!(line(&out, "RAND|"), line(&other, "RAND|"));

    let (out, err) = run(&d, &so, "tb.sv", &["+seed=random"]);
    assert_eq!(vpi_plusargs(&out), ["+seed=random"]);
    assert_eq!(line(&out, "SV_VALUE|"), "SV_VALUE|random");
    let seed = drawn_seed(&err);
    assert!(
        err.contains(&format!("replay with +seed={}", seed)),
        "{}",
        err
    );
    let (replay, _) = run(&d, &so, "tb.sv", &[&format!("+seed={}", seed)]);
    assert_eq!(line(&replay, "RAND|"), line(&out, "RAND|"));
}

/// Given both, the option seeds the RNG and the plusarg is left to the
/// testbench, as on the reference simulator.
#[test]
fn seed_option_wins_over_seed_plusarg() {
    let (d, so) = setup("both");
    let (both, _) = run(&d, &so, "tb.sv", &["-sv_seed", "7", "+seed=123"]);
    let (opt, _) = run(&d, &so, "tb.sv", &["-sv_seed", "7"]);
    let (plus, _) = run(&d, &so, "tb.sv", &["+seed=123"]);
    assert_eq!(line(&both, "RAND|"), line(&opt, "RAND|"));
    assert_ne!(line(&both, "RAND|"), line(&plus, "RAND|"));
    assert_eq!(vpi_plusargs(&both), ["+seed=123"]);
    assert_eq!(line(&both, "SV_VALUE|"), "SV_VALUE|123");
}

/// A design saved with `--compile` and run later takes the option the same way.
#[test]
fn seed_option_on_a_compiled_design() {
    let (d, so) = setup("compiled");
    let (code, out, err) = xezim(&d, &["--compile", "-o", "tb.xbc", "tb.sv"]);
    assert_eq!(code, 0, "{}{}", out, err);
    let (native, _) = run(&d, &so, "tb.xbc", &["+seed=123"]);
    let (out, _) = run(&d, &so, "tb.xbc", &["-sv_seed", "123"]);
    assert!(vpi_plusargs(&out).is_empty(), "{}", out);
    assert_eq!(line(&out, "SV_TEST|"), "SV_TEST|0");
    assert_eq!(line(&out, "RAND|"), line(&native, "RAND|"));
}
