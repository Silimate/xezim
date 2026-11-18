//! IEEE 1800-2023 §23.2.2.3: an ANSI output port declared with no data type
//! (`output q`, `output [3:0] cnt`) is a net of the default net type, and §6.5
//! forbids a procedural assignment to a net. OpenHW CORE-ET's
//! `txfmactl_top` declares an output that way and assigns it in an
//! `always_ff`; Verilator accepts that and takes the port as a variable, and
//! the reference simulator rejects it. The error now says the port is a net
//! because its declaration has no data type and suggests `output logic`, and
//! `--relax-untyped-output-port` (or `XEZIM_RELAX_UNTYPED_OUTPUT_PORT=1`) takes
//! such a port as a variable, with a warning. A port declared as a net
//! (`output wire w`) stays an error.
use std::path::Path;
use std::process::Command;

fn run_in(dir: &Path, source: &str, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("repro.sv");
    std::fs::write(&path, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["-s", "t"])
        .args(args)
        .arg(&path)
        .env_remove("XEZIM_RELAX_UNTYPED_OUTPUT_PORT")
        .env_remove("XEZIM_ENABLE_CACHE")
        .envs(env.iter().copied())
        .output()
        .expect("run xezim");
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("xezim_untyped_out_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn run(tag: &str, source: &str, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
    let d = scratch(tag);
    let mut a = vec!["--no-cache"];
    a.extend_from_slice(args);
    let r = run_in(&d, source, &a, env);
    let _ = std::fs::remove_dir_all(&d);
    r
}

/// `q` and `cnt` have no data type, and `r` inherits that from `q`
/// (§23.2.2.3); `v` is a variable, and so is `u`, which inherits `logic`
/// from `v`.
const FLOP: &str = r#"
module flop (input logic clk, input logic rst, input logic d,
             output q, r, output [3:0] cnt, output logic v, u);
  always_ff @(posedge clk)
    if (rst) begin q <= 1'b0; cnt <= 4'd0; v <= 1'b0; r <= 1'b1; u <= 1'b0; end
    else begin q <= d; cnt <= cnt + 4'd1; v <= ~v; r <= ~d; u <= d; end
endmodule
module t;
  logic clk = 0, rst = 1, d = 1;
  wire q, v, r, u;
  wire [3:0] cnt;
  flop dut (.clk, .rst, .d, .q, .cnt, .v, .r, .u);
  always #5 clk = ~clk;
  initial begin
    #12 rst = 0; #30;
    $display("R|q=%b cnt=%0d v=%b r=%b u=%b", q, cnt, v, r, u);
    $finish;
  end
endmodule
"#;

/// What `FLOP` prints once its untyped ports are taken as variables. Edges at
/// 5 (reset), 15, 25 and 35; the display is at 42.
const FLOP_OUT: &str = "R|q=1 cnt=3 v=1 r=0 u=1";

#[test]
fn untyped_output_port_error_says_it_is_a_net_and_how_to_declare_it() {
    let (ok, text) = run("err", FLOP, &[], &[]);
    assert!(!ok, "accepted:\n{text}");
    for port in ["q", "cnt", "r"] {
        let want = format!(
            "output port '{port}' of module 'flop' is the target of a procedural assignment"
        );
        assert!(text.contains(&want), "missing {want:?}:\n{text}");
    }
    for want in [
        "Declare it `output logic` rather than `output`",
        "its declaration has no data type",
        "§23.2.2.3",
        "§6.5",
        "--relax-untyped-output-port",
    ] {
        assert!(text.contains(want), "missing {want:?}:\n{text}");
    }
    // `output logic v, u` are variables.
    for port in ["v", "u"] {
        assert!(
            !text.contains(&format!("port '{port}'")),
            "a typed port was flagged:\n{text}"
        );
    }
}

#[test]
fn relaxation_takes_the_port_as_a_variable() {
    for (args, env) in [
        (&["--relax-untyped-output-port"][..], &[][..]),
        (&[][..], &[("XEZIM_RELAX_UNTYPED_OUTPUT_PORT", "1")][..]),
    ] {
        let (ok, text) = run("relax", FLOP, args, env);
        assert!(ok, "{args:?} {env:?}:\n{text}");
        assert!(text.contains(FLOP_OUT), "{args:?} {env:?}:\n{text}");
        for port in ["q", "cnt", "r"] {
            let want =
                format!("[xezim][warning] output port '{port}' of module 'flop' has no data type");
            assert!(text.contains(&want), "missing {want:?}:\n{text}");
        }
        assert!(
            text.contains("as if it were declared `output logic`"),
            "{args:?} {env:?}:\n{text}"
        );
    }
}

/// A port declared as a net keeps the plain net error, relaxed or not, and so
/// does `x`, which inherits `wire` from `w`.
#[test]
fn an_explicit_net_port_stays_an_error() {
    let src = r#"
module m (input logic clk, output wire w, x);
  always @(posedge clk) begin w <= 1'b1; x <= 1'b0; end
endmodule
module t;
  logic clk = 0;
  wire w, x;
  m u (.clk, .w, .x);
  initial $finish;
endmodule
"#;
    for args in [&[][..], &["--relax-untyped-output-port"][..]] {
        let (ok, text) = run("wire", src, args, &[]);
        assert!(!ok, "{args:?} accepted:\n{text}");
        for port in ["w", "x"] {
            let want = format!("net '{port}' is the target of a procedural assignment");
            assert!(text.contains(&want), "{args:?} missing {want:?}:\n{text}");
        }
        assert!(!text.contains("no data type"), "{args:?}:\n{text}");
    }
}

#[test]
fn help_lists_the_relaxation() {
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .arg("--help")
        .output()
        .expect("run xezim");
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--relax-untyped-output-port"), "{text}");
    assert!(text.contains("XEZIM_RELAX_UNTYPED_OUTPUT_PORT=1"), "{text}");
}

/// A warm design-cache hit, which skips the lint, replays its warnings: each
/// once, as on the cold run.
#[test]
fn a_warm_cache_hit_replays_the_warnings() {
    let d = scratch("warm");
    let cache = d.join("cache");
    let cache = cache.to_str().unwrap();
    let args = [
        "--cache-dir",
        cache,
        "--verbose",
        "--relax-untyped-output-port",
    ];
    for (run_no, want_cache) in [(1, "CACHE] miss"), (2, "CACHE] hit")] {
        let (ok, text) = run_in(&d, FLOP, &args, &[]);
        assert!(ok, "run {run_no}:\n{text}");
        assert!(text.contains(want_cache), "run {run_no}:\n{text}");
        assert!(text.contains(FLOP_OUT), "run {run_no}:\n{text}");
        for port in ["q", "cnt", "r"] {
            let want =
                format!("[xezim][warning] output port '{port}' of module 'flop' has no data type");
            assert_eq!(
                text.matches(&want).count(),
                1,
                "run {run_no}: {want:?} not printed once:\n{text}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The relaxation is part of the design-cache key: a strict run never reuses
/// the design a relaxed run cached.
#[test]
fn a_strict_run_does_not_reuse_a_relaxed_design() {
    let d = scratch("cache");
    let cache = d.join("cache");
    let cache = cache.to_str().unwrap();
    let (ok, text) = run_in(
        &d,
        FLOP,
        &["--cache-dir", cache, "--relax-untyped-output-port"],
        &[],
    );
    assert!(ok, "relaxed:\n{text}");
    let (ok, text) = run_in(&d, FLOP, &["--cache-dir", cache], &[]);
    assert!(!ok, "a strict run took the relaxed design:\n{text}");
    assert!(
        text.contains("output port 'q' of module 'flop' is the target"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&d);
}
