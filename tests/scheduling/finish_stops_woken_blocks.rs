//! §20.2: `$finish` ends the run, so an always block woken in the finishing
//! time slot either runs whole before the finish or not at all. Compiled
//! block code used to ignore the finish: the block's compiled statements ran
//! (counters, NBAs) while its interpreted ones (`$display`) were skipped, so
//! the `final` block reported a value the log never showed.
//!
//! The reference simulator runs no woken block after the finish in either
//! shape below.

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
