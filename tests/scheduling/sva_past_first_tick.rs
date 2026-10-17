//! §16.9.3 — sampled-value functions at a clock tick before the first one.
//!
//! When fewer than `number_of_ticks` ticks have occurred, `$past` returns the
//! DEFAULT SAMPLED VALUE of its operand, and at the first tick `$rose`,
//! `$fell`, `$stable` and `$changed` compare the sampled value with it.
//! §16.5.1 defines that value as the one before time 0: a static variable's
//! declaration initializer, else its type's default (x for `logic`, 0 for
//! `bit`), and the type's default for a net. A write at time 0 does not count,
//! but an initializer xezim runs as a time-0 process (`fv` below) does.
//!
//! A concurrent assertion read x there for every operand and treated the
//! value-change functions as "no change", so an unguarded
//! `q == $past(d)` with `logic d = 0` failed at the first edge. The sampled
//! value compared at that tick is the Preponed one: an operand read only
//! inside sampled-value calls (`t1` below) used to be read after the tick's
//! NBAs, so `$rose(t1)` fired one tick early and `$past(t1)` was the
//! current tick's value.
//!
//! The reference simulator, run with full visibility (`+acc`), reports only
//! the `FIRST 5` line here. In its default optimized mode it folds the net
//! `w` into `r`'s initial value.

use std::path::PathBuf;
use std::process::Command;

fn run(name: &str, src: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sva_past_first_tick");
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
    text
}

#[test]
fn sampled_value_functions_use_default_sampled_values_before_the_first_tick() {
    let text = run(
        "defaults",
        r#"
module t;
  logic clk = 0;
  always #5 clk = ~clk;
  logic first = 1;                 // sampled as 1 at the first tick only
  always @(posedge clk) first <= 0;
  logic d = 0, q = 0, q2 = 0;
  always_ff @(posedge clk) begin q <= d; q2 <= q; end
  logic [7:0] v = 8'hA5;           // declaration initializer
  logic u;                         // no initializer: logic's default, x
  bit b;                           // no initializer: bit's default, 0
  int n = -7;
  logic r = 0, f = 1, i;           // all three written at time 0 below
  wire w; assign w = r;            // a net: its type's default, x
  logic en = 0;
  function automatic logic [7:0] triple(input int x); return 8'(x * 3); endfunction
  logic [7:0] fv = triple(3);      // an initializer run as a time-0 process
  int k = 0;                       // sampled as the number of earlier ticks
  logic t1 = 0;                    // sampled as 1 from the second tick on
  always_ff @(posedge clk) begin k <= k + 1; t1 <= 1; end
  initial begin
    r = 1; f = 0; i = 1;
    @(negedge clk) d = 1;
    @(negedge clk) d = 0; en = 1;
    repeat (3) @(negedge clk);
    $finish;
  end
  a_first: assert property (@(posedge clk) !first) else $display("FIRST %0t", $time);
  // The unguarded check of the report.
  a_q:  assert property (@(posedge clk) q == $past(d)) else $display("FAIL q %0t", $time);
  a_v:  assert property (@(posedge clk) $past(v) === 8'hA5) else $display("FAIL v %0t", $time);
  a_fv: assert property (@(posedge clk) $past(fv) === 8'd9 && $stable(fv)) else $display("FAIL fv %0t", $time);
  a_u:  assert property (@(posedge clk) $past(u) === 1'bx) else $display("FAIL u %0t", $time);
  a_b:  assert property (@(posedge clk) $past(b) === 1'b0) else $display("FAIL b %0t", $time);
  a_n:  assert property (@(posedge clk) $past(n) == -7) else $display("FAIL n %0t", $time);
  a_i:  assert property (@(posedge clk) $past(i) === (first ? 1'bx : 1'b1)) else $display("FAIL i %0t", $time);
  a_w:  assert property (@(posedge clk) $past(w) === (first ? 1'bx : 1'b1)) else $display("FAIL w %0t", $time);
  // Two ticks back: the default until a second tick has passed.
  a_d2: assert property (@(posedge clk) $past(d, 2) === q2) else $display("FAIL d2 %0t", $time);
  // Gated: no tick with `en` true yet.
  a_g:  assert property (@(posedge clk) $past(v, 1, en) === 8'hA5) else $display("FAIL g %0t", $time);
  a_rr: assert property (@(posedge clk) $rose(r) === first) else $display("FAIL rose_r %0t", $time);
  a_ri: assert property (@(posedge clk) $rose(i) === first) else $display("FAIL rose_i %0t", $time);
  a_rw: assert property (@(posedge clk) $rose(w) === first) else $display("FAIL rose_w %0t", $time);
  a_ff: assert property (@(posedge clk) $fell(f) === first) else $display("FAIL fell_f %0t", $time);
  a_sv: assert property (@(posedge clk) $stable(v)) else $display("FAIL stable_v %0t", $time);
  a_sr: assert property (@(posedge clk) $stable(r) === !first) else $display("FAIL stable_r %0t", $time);
  a_ci: assert property (@(posedge clk) $changed(i) === first) else $display("FAIL changed_i %0t", $time);
  // `t1` is written by an NBA at each tick and read only through $rose/$past.
  a_rt: assert property (@(posedge clk) $rose(t1) === (k == 1)) else $display("FAIL rose_t1 %0t", $time);
  a_pt: assert property (@(posedge clk) $past(t1) === (k >= 2)) else $display("FAIL past_t1 %0t", $time);
  // Procedural calls read the same default as the assertions.
  always @(posedge clk) begin
    if ($past(w) !== (first ? 1'bx : 1'b1) || $rose(w) !== first)
      $display("FAIL proc_w %0t", $time);
    if ($past(fv) !== 8'd9 || $changed(fv))
      $display("FAIL proc_fv %0t", $time);
  end
endmodule
"#,
    );
    assert!(text.contains("$finish called"), "did not finish:\n{text}");
    let reports: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("FAIL") || l.starts_with("FIRST"))
        .collect();
    assert_eq!(reports, ["FIRST 5"], "report lines:\n{text}");
}

/// An assertion written inline in a procedure under a `wait` reads the same
/// defaults. The scan that saves them before time 0 skipped the body of a
/// `wait`, so `$past(b)` there read x at the first tick (§16.9.3, §16.14.6).
#[test]
fn inline_assertion_under_wait_uses_default_sampled_values() {
    let text = run(
        "wait_inline",
        r#"
module t;
  logic clk = 0;
  always #5 clk = ~clk;
  bit b = 0;
  logic [3:0] v = 4'h9;
  initial wait (1)
    a_wb: assert property (@(posedge clk) $past(b) === 1'b0) else $display("FAIL wait_b %0t", $time);
  initial fork
    wait (clk == 0) a_wv: assert property (@(posedge clk) $past(v) === 4'h9 && !$rose(v[0])) else $display("FAIL wait_v %0t", $time);
  join_none
  initial begin repeat (3) @(negedge clk); $finish; end
endmodule
"#,
    );
    assert!(text.contains("$finish called"), "did not finish:\n{text}");
    let fails: Vec<&str> = text.lines().map(str::trim).filter(|l| l.starts_with("FAIL")).collect();
    assert!(fails.is_empty(), "report lines:\n{text}");
}
