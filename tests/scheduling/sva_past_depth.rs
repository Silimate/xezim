//! §16.9.3 — `$past(expr, number_of_ticks)` for any number of ticks.
//!
//! `number_of_ticks` is any elaboration-time constant ≥ 1; the LRM sets no
//! upper limit. A concurrent assertion kept 16 samples per `$past` operand,
//! the current one included, so `$past(x, N)` was the default value (x) for
//! every N ≥ 16 however many ticks had passed. A procedural `$past(x, N)` kept
//! the history its first evaluation asked for, so a call first reached after
//! N edges had only the last edge to read.
//!
//! Each history is now as long as the deepest tick count read from it, found
//! once at elaboration: a parameter gives each instance its own depth, and a
//! shallow assertion keeps no more than it reads. The expected lines are the
//! reference simulator's.

use std::path::PathBuf;
use std::process::Command;

fn run(name: &str, src: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sva_past_depth");
    std::fs::create_dir_all(&dir).unwrap();
    let sv = dir.join(format!("{name}.sv"));
    std::fs::write(&sv, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--simulate", "-s", "t", "--no-cache", sv.to_str().unwrap()])
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "run failed:\n{text}");
    assert!(!text.contains("FAIL"), "assertion failed:\n{text}");
    text
}

fn result(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with("RESULT"))
        .unwrap_or_else(|| panic!("no RESULT line:\n{text}"))
}

/// 100 ticks; `k` is the number of earlier ticks and `st` is sampled as `k`,
/// so `$past(st, N) == st - N` holds at the `100 - N` ticks with `k >= N` and
/// at no earlier one.
#[test]
fn concurrent_past_reads_any_number_of_ticks_back() {
    let text = run(
        "concurrent",
        r#"
module chk #(parameter int N = 1) (input logic clk, input logic [31:0] st, input int k);
  int hits = 0;
  a: assert property (@(posedge clk) k >= N |-> $past(st, N) == st - N)
       else $display("FAIL chk N=%0d k=%0d", N, k);
  c: cover property (@(posedge clk) k >= N && $past(st, N) == st - N) hits++;
endmodule

module t;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [31:0] st = 0;
  int k = 0;
  always_ff @(posedge clk) begin st <= st + 1; k <= k + 1; end
  localparam int D = 3 * 7;
  int h15 = 0, h16 = 0, h17 = 0, h64 = 0, hd = 0, hp = 0, hm = 0, hr = 0;
  c15: cover property (@(posedge clk) k >= 15 && $past(st, 15) == st - 15) h15++;
  c16: cover property (@(posedge clk) k >= 16 && $past(st, 16) == st - 16) h16++;
  c17: cover property (@(posedge clk) k >= 17 && $past(st, 17) == st - 17) h17++;
  c64: cover property (@(posedge clk) k >= 64 && $past(st, 64) == st - 64) h64++;
  cd:  cover property (@(posedge clk) k >= D && $past(st, D) == st - D) hd++;
  // Through a property argument.
  property lat(int d); @(posedge clk) k >= d && $past(st, d) == st - d; endproperty
  cp:  cover property (lat(33)) hp++;
  // Two depths of one operand in one assertion, and a value-change function.
  cm:  cover property (@(posedge clk) k >= 50 && $past(st, 3) + $past(st, 50) == 2 * st - 53
                       && $changed(st)) hm++;
  // A deep history of an operand $rose reads too; `b` is sampled as k[0].
  logic b = 0;
  always @(negedge clk) b <= st[0];
  cr:  cover property (@(posedge clk) k >= 31 && $rose(b) && $past(b, 31) == 1'b0) hr++;
  // A parameter gives each instance its own depth.
  chk #(.N(20)) u20 (.clk, .st, .k);
  chk #(.N(40)) u40 (.clk, .st, .k);
  initial begin
    #1000;
    $display("RESULT h15=%0d h16=%0d h17=%0d h64=%0d hd=%0d hp=%0d hm=%0d hr=%0d u20=%0d u40=%0d",
             h15, h16, h17, h64, hd, hp, hm, hr, u20.hits, u40.hits);
    $finish;
  end
endmodule
"#,
    );
    assert_eq!(
        result(&text),
        "RESULT h15=85 h16=84 h17=83 h64=36 hd=79 hp=67 hm=50 hr=35 u20=80 u40=60",
        "{text}"
    );
}

/// A `$past` inside a concatenation, a replication or a sequence match item
/// sizes its operand's history too. Inside a concatenation it shared a
/// two-sample history with the `$changed` of the same operand.
#[test]
fn past_in_any_container_sizes_the_history() {
    let text = run(
        "containers",
        r#"
module t;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [31:0] st = 0;
  int k = 0;
  always_ff @(posedge clk) begin st <= st + 1; k <= k + 1; end
  int hc = 0, hn = 0, hv = 0;
  cc: cover property (@(posedge clk) k >= 5 && {$past(st, 5)} == st - 5 && $changed(st)) hc++;
  cn: cover property (@(posedge clk) k >= 20 && {2{$past(st, 20)}} == {2{st - 20}} && !$stable(st)) hn++;
  property mi;
    logic [31:0] v;
    @(posedge clk) (k >= 25, v = $past(st, 25)) ##0 (v == st - 25);
  endproperty
  cv: cover property (mi) hv++;
  initial begin
    #1000;
    $display("RESULT hc=%0d hn=%0d hv=%0d", hc, hn, hv);
    $finish;
  end
endmodule
"#,
    );
    assert_eq!(result(&text), "RESULT hc=95 hn=80 hv=75", "{text}");
}

/// A procedural `$past` first evaluated at the 71st tick reads 20 (or N)
/// ticks back all the same.
#[test]
fn procedural_past_keeps_its_ticks_from_the_first_edge() {
    let text = run(
        "procedural",
        r#"
module pchk #(parameter int N = 1) (input logic clk, input logic [31:0] st, input int k);
  logic [31:0] pv = '1;
  always @(posedge clk) if (k == 70) pv <= $past(st, N);
endmodule

module t;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [31:0] st = 0;
  int k = 0;
  always_ff @(posedge clk) begin st <= st + 1; k <= k + 1; end
  logic [31:0] pv = '1, pw = '1;
  always @(posedge clk) if (k == 70) pv <= $past(st, 20);
  always @(posedge clk) if (k == 70) pw <= $past(st, 20, 1'b1, @(posedge clk));
  pchk #(.N(30)) u30 (.clk, .st, .k);
  pchk #(.N(45)) u45 (.clk, .st, .k);
  initial begin
    #1000;
    $display("RESULT pv=%0d pw=%0d u30=%0d u45=%0d", pv, pw, u30.pv, u45.pv);
    $finish;
  end
endmodule
"#,
    );
    assert_eq!(result(&text), "RESULT pv=50 pw=50 u30=40 u45=25", "{text}");
}
