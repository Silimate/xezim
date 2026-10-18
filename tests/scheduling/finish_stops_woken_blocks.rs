//! §20.2: `$finish` ends the run, so an always block woken in the finishing
//! time slot either runs whole before the finish or not at all. Compiled
//! block code used to ignore the finish: the block's compiled statements ran
//! (counters, NBAs) while its interpreted ones (`$display`) were skipped, so
//! the `final` block reported a value the log never showed. And a finish
//! from an always block or a process was deferred to the end of the slot
//! like a resumed waiter's, so later passes ran more blocks.
//!
//! The reference simulator runs no block after the finish in any shape
//! below.

use xezim::simulate;

fn lines(src: &str) -> Vec<String> {
    let sim = simulate(src, 1_000).expect("simulate failed");
    sim.output
        .iter()
        .map(|o| o.message.trim().to_string())
        .filter(|l| l.starts_with("RAN") || l.starts_with("FINAL"))
        .collect()
}

/// `$finish` from one always block, a second woken by the same edge. Their
/// order is free (§4.7), but the `final` block must agree with the last
/// line the second block printed.
#[test]
fn finish_from_edge_block_skips_or_completes_peer() {
    let got = lines(
        r#"
module top;
  bit clk = 0;
  int n = 0;
  initial forever #5 clk = ~clk;
  always @(posedge clk) if (n == 2) $finish;
  always @(posedge clk) begin n++; $display("RAN n=%0d", n); end
  final $display("FINAL n=%0d", n);
endmodule
"#,
    );
    let last = got.iter().rev().find(|l| l.starts_with("RAN")).unwrap();
    let fin = got.last().unwrap();
    assert_eq!(
        fin.trim_start_matches("FINAL"),
        last.trim_start_matches("RAN"),
        "{got:?}"
    );
}

/// An always block writes `q` and calls `$finish`. The block woken by `q`
/// must not run in a later pass of the same slot: the finish of an always
/// block is not the deferred finish of a resumed waiter (reference
/// simulator: hits=0).
#[test]
fn finish_from_edge_block_stops_later_passes() {
    let got = lines(
        r#"
module top;
  reg clk = 0, q = 0;
  int hits = 0;
  always #5 clk = ~clk;
  always @(posedge clk) if ($time > 10) begin q = 1; $finish; end
  always @(posedge q) begin hits++; $display("RAN hits=%0d", hits); end
  final $display("FINAL hits=%0d", hits);
endmodule
"#,
    );
    assert_eq!(got, ["FINAL hits=0"]);
}

/// A process writes the clock and calls `$finish` without suspending: the
/// blocks it woke never get to run (reference simulator: edges=1 nb=1).
#[test]
fn finish_after_clock_write_runs_no_woken_block() {
    let got = lines(
        r#"
module top;
  reg clk = 0;
  int edges = 0, nb = 0;
  initial begin #5 clk = 1; #5 clk = 0; #5 clk = 1; $finish; end
  always @(posedge clk) begin edges++; $display("RAN edges=%0d", edges); end
  always @(posedge clk) nb <= nb + 1;
  final $display("FINAL edges=%0d nb=%0d", edges, nb);
endmodule
"#,
    );
    assert_eq!(got, ["RAN edges=1", "FINAL edges=1 nb=1"]);
}

/// A process resumed by the clock edge calls `$finish`, the way a stimulus
/// testbench ends (`repeat (n) @(posedge clk); $finish;`). The edge's always
/// blocks may run before it (§4.7 leaves the order free), but the slot never
/// reaches its NBA, Observed or Postponed region (§4.4.2, §20.2): no NBA
/// commits, no assertion is evaluated and no `$strobe` prints at t=25. The
/// reference simulator prints edges=3 nb=2 and nothing for the assertion or
/// the `$strobe` at t=25; xezim used to defer this finish to the end of the
/// slot (nb=3, both lines at t=25).
#[test]
fn finish_from_resumed_waiter_skips_later_regions() {
    let sim = simulate(
        r#"
module top;
  reg clk = 0;
  int edges = 0, nb = 0;
  always #5 clk = ~clk;
  initial begin repeat (3) @(posedge clk); $finish; end
  always @(posedge clk) begin edges++; $display("RAN edges=%0d", edges); end
  always @(posedge clk) nb <= nb + 1;
  always @(posedge clk) $strobe("strobe t=%0t", $time);
  a_on: assert property (@(posedge clk) 1) $display("assert t=%0t", $time);
  final $display("FINAL edges=%0d nb=%0d", edges, nb);
endmodule
"#,
        1_000,
    )
    .expect("simulate failed");
    let got: Vec<String> = sim
        .output
        .iter()
        .map(|o| o.message.trim().to_string())
        .collect();
    let ran = got.iter().filter(|l| l.starts_with("RAN")).count();
    assert_eq!(
        got.iter().rev().find(|l| l.starts_with("FINAL")),
        Some(&format!("FINAL edges={ran} nb=2")),
        "{got:?}"
    );
    assert_eq!(
        got.iter().filter(|l| l.starts_with("assert")).count(),
        2,
        "{got:?}"
    );
    assert_eq!(
        got.iter().filter(|l| l.starts_with("strobe")).count(),
        2,
        "{got:?}"
    );
}
