//! `--compat` (or `XEZIM_COMPAT=1`): what commercial simulators do by default,
//! in one flag, for flows written for them. It is exactly
//! `--max-time none --relax-implicit-static --relax-driver-conflicts
//! --error-exit`, except that an explicit `--max-time` (or a `-do` run time)
//! still sets the run length. Also `--max-time none` / `0`, which lifts the
//! default 100 ms cap: the run goes until `$finish` or no events remain, as
//! on the reference simulator.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xezim_compat_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run xezim in `dir` with `env` set; returns (exit code, stdout, stderr).
fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .current_dir(dir)
        .env_remove("XEZIM_COMPAT")
        .env_remove("XEZIM_ALLOW_IMPLICIT_STATIC")
        .env_remove("XEZIM_RELAX_DRIVER_CONFLICTS")
        .envs(env.iter().copied())
        .args(args)
        .output()
        .expect("run xezim");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `q` is written by an `always_ff` and by an `initial` block (§9.2.2.4),
/// `n` is an implicitly static variable with an initializer in a static
/// function (§6.21), and `$finish` comes at 150 ms, past the default cap.
/// The reference simulator reports both as suppressible errors; with them
/// suppressed it runs to `$finish` and prints `late t=150000030 q=3
/// count=1`. (A second call returns 2 there, as §6.21 requires. xezim's
/// relaxed initializer runs on every call, a separate bug, so the test
/// calls once.)
const LENIENT: &str = "\
module tb;
  logic clk = 0;
  logic [3:0] q;
  always_ff @(posedge clk) q <= q + 1;
  initial q = 0;
  function int count();
    int n = 0;
    n++;
    return n;
  endfunction
  initial begin
    repeat (3) begin #5 clk = 1; #5 clk = 0; end
    #150ms;
    $display(\"late t=%0t q=%0d count=%0d\", $time, q, count());
    $finish;
  end
endmodule
";

const LATE: &str = "late t=150000030 q=3 count=1";

#[test]
fn compat_is_the_four_switches() {
    let d = scratch("four");
    std::fs::write(d.join("tb.sv"), LENIENT).unwrap();
    // Without it, the §6.21 initializer is an error.
    let (code, _, err) = run(&d, &["tb.sv"], &[]);
    assert_eq!(code, 1, "{}", err);
    assert!(err.contains("implicitly static"), "{}", err);
    let each = run(
        &d,
        &[
            "--relax-implicit-static",
            "--relax-driver-conflicts",
            "--error-exit",
            "--max-time",
            "none",
            "tb.sv",
        ],
        &[],
    );
    assert_eq!(each.0, 0, "{}", each.2);
    assert!(each.1.contains(LATE), "{}", each.1);
    assert!(each.1.contains("$finish called"), "{}", each.1);
    // Both relaxed checks still warn.
    for warning in [
        "accepted because --relax-implicit-static is in effect",
        "accepted because --relax-driver-conflicts is in effect",
    ] {
        assert!(each.2.contains(warning), "{}", each.2);
    }
    assert_eq!(run(&d, &["--compat", "tb.sv"], &[]), each);
    assert_eq!(run(&d, &["tb.sv", "--compat"], &[]), each);
    assert_eq!(run(&d, &["tb.sv"], &[("XEZIM_COMPAT", "1")]), each);
    std::fs::write(d.join("files.f"), "--compat\ntb.sv\n").unwrap();
    assert_eq!(run(&d, &["-f", "files.f"], &[]), each);
    let _ = std::fs::remove_dir_all(&d);
}

/// §20.10: `$error` does not end the run, and xezim exits 0 after it unless
/// asked; `--compat` asks, as `--error-exit` does, so a flow that judges a
/// run by its exit status sees the failure.
#[test]
fn compat_fails_a_run_with_an_error() {
    let d = scratch("error");
    std::fs::write(
        d.join("e.sv"),
        "module tb; initial begin $error(\"boom\"); #1 $display(\"after\"); end endmodule\n",
    )
    .unwrap();
    let (code, out, _) = run(&d, &["e.sv"], &[]);
    assert_eq!(code, 0);
    assert!(out.contains("after"), "{}", out);
    let (code, out, _) = run(&d, &["--compat", "e.sv"], &[]);
    assert_eq!(code, 1);
    assert!(out.contains("after"), "the run carries on:\n{}", out);
    let (code, _, _) = run(&d, &["e.sv"], &[("XEZIM_COMPAT", "1")]);
    assert_eq!(code, 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// An explicit `--max-time`, before or after `--compat`, and a `-do` run
/// time still set the run length.
#[test]
fn compat_keeps_an_explicit_run_length() {
    let d = scratch("length");
    std::fs::write(d.join("tb.sv"), LENIENT).unwrap();
    for args in [
        &["--compat", "--max-time", "1ms", "tb.sv"][..],
        &["--max-time=1ms", "--compat", "tb.sv"][..],
    ] {
        let (code, out, err) = run(&d, args, &[]);
        assert_eq!(code, 0, "{:?}: {}", args, err);
        assert!(!out.contains("late"), "{:?}: {}", args, out);
        assert!(!out.contains("$finish called"), "{:?}: {}", args, out);
    }
    let (code, out, err) = run(&d, &["--compat", "tb.sv", "-do", "run 100ns"], &[]);
    assert_eq!(code, 0, "{}", err);
    assert!(
        out.ends_with("Simulation finished at time 100\n"),
        "{}",
        out
    );
    let (code, out, err) = run(&d, &["--compat", "tb.sv", "-do", "run -all"], &[]);
    assert_eq!(code, 0, "{}", err);
    assert!(out.contains(LATE), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}

/// `--max-time none` and `--max-time 0` (in any unit) lift the default
/// 100 ms cap; a negative value is still refused.
#[test]
fn max_time_none_runs_to_finish() {
    let d = scratch("none");
    std::fs::write(
        d.join("tb.sv"),
        "module tb; initial begin #150ms $display(\"L t=%0t\", $time); $finish; end endmodule\n",
    )
    .unwrap();
    let (code, capped, _) = run(&d, &["tb.sv"], &[]);
    assert_eq!(code, 0);
    assert!(!capped.contains("L t="), "{}", capped);
    let (code, all, err) = run(&d, &["tb.sv", "-do", "run -all"], &[]);
    assert_eq!(code, 0, "{}", err);
    assert!(all.contains("L t=150000000\n"), "{}", all);
    for v in ["none", "NONE", "0", "0ns", "0s"] {
        let (code, out, err) = run(&d, &["tb.sv", "--max-time", v], &[]);
        assert_eq!((code, out.as_str()), (0, all.as_str()), "{}: {}", v, err);
        let (code, out, _) = run(&d, &[&format!("--max-time={}", v), "tb.sv"], &[]);
        assert_eq!((code, out.as_str()), (0, all.as_str()), "={}", v);
    }
    let (code, _, err) = run(&d, &["tb.sv", "--max-time", "-5"], &[]);
    assert_eq!(code, 1);
    assert!(err.contains("0/none"), "{}", err);
    let _ = std::fs::remove_dir_all(&d);
}
