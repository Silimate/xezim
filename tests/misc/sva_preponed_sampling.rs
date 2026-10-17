//! LRM §16.5.1 / §16.9 — a clocked concurrent assertion must evaluate its
//! predicate against values sampled in the PREPONED region (the start of the
//! clock-tick time slot, BEFORE the active/NBA updates of that slot). A signal
//! written via NBA on the SAME edge the assertion is clocked on must therefore
//! be seen with its OLD value, not the just-committed one.
//!
//! Before this fix xezim evaluated the predicate against post-NBA (live)
//! values, so the assertion was effectively one clock cycle early. The
//! expectations below are locked to a commercial simulator: exactly one
//! failure, at time 5, for `a |-> b` when `b <= a` fires on the same posedge.

use xezim::simulate;

fn out(src: &str) -> String {
    let sim = simulate(src, 1000).expect("simulate failed");
    sim.output
        .iter()
        .map(|o| o.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `b` is NBA-assigned `a` on the posedge. The property `a |-> b` samples the
/// PREPONED `b` (still 0 on the first edge) and so must FAIL exactly once, at
/// time 5, then pass on every later edge (preponed b == 1 thereafter).
#[test]
fn preponed_nba_same_edge_fails_once() {
    let o = out(r#"
module top;
  logic clk = 0;
  logic a = 1'b1;
  logic b = 1'b0;
  int   fails = 0;
  always #5 clk = ~clk;
  always @(posedge clk) b <= a;   // NBA on the same edge the assertion fires
  ap: assert property (@(posedge clk) a |-> b)
      else begin
        fails = fails + 1;
        $display("[ASSERT-FAIL] time=%0t", $time);
      end
  initial begin #52; $display("[RESULT] total_fails=%0d", fails); $finish; end
endmodule
"#);
    // Exactly one failure, and it happens at the first posedge (time 5).
    assert!(
        o.contains("[RESULT] total_fails=1"),
        "preponed sampling must fail exactly once (was 0 with post-NBA sampling); got: {}",
        o
    );
    assert!(
        o.contains("[ASSERT-FAIL] time=5"),
        "the single failure must fire at time 5 (the first posedge); got: {}",
        o
    );
}

/// Control: when `b` is a plain register that already holds 1 before the edge
/// (no same-edge write), `a |-> b` never fails — confirms the preponed override
/// is a no-op for signals that do not change on the clock edge.
#[test]
fn preponed_stable_signal_never_fails() {
    let o = out(r#"
module top;
  logic clk = 0;
  logic a = 1'b1;
  logic b = 1'b1;
  int   fails = 0;
  always #5 clk = ~clk;
  ap: assert property (@(posedge clk) a |-> b) else fails = fails + 1;
  initial begin #52; $display("[RESULT] total_fails=%0d", fails); $finish; end
endmodule
"#);
    assert!(
        o.contains("[RESULT] total_fails=0"),
        "a stable true signal must never fail the implication; got: {}",
        o
    );
}

/// §16.9.3: the argument of a sampled-value function is sampled in the
/// Preponed region too. For a flop on the property's own clock, `$past(q)`,
/// `$rose(q)`, `$fell(q)`, `$stable(q)` and `$changed(q)` see `q` from before
/// the edge that updates it. Its history used to record the value the flop
/// took at the edge, so `$past(q)` returned the current sample and `$rose`
/// fired a cycle early. The commercial simulator: 6 changes, no failure.
#[test]
fn sampled_value_functions_read_preponed_flop_values() {
    let o = out(r#"
module top;
  logic clk = 0, rst = 1, d = 0, q = 0, q2 = 0;
  int fails = 0, n = 0;
  bit pat[12] = '{0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1};
  always #5 clk = ~clk;
  always_ff @(posedge clk) begin q <= d; q2 <= q; end
  initial begin
    foreach (pat[i]) begin
      @(negedge clk);
      d = pat[i];
      if (i == 1) rst = 0;
    end
    @(negedge clk);
    $display("[RESULT] changed=%0d fails=%0d", n, fails);
    $finish;
  end
  a_past: assert property (@(posedge clk) disable iff (rst) $past(q) == q2)
    else begin fails++; $display("[FAIL] a_past t=%0t", $time); end
  a_rose: assert property (@(posedge clk) disable iff (rst) $rose(q) |-> !q2 && $past(d))
    else begin fails++; $display("[FAIL] a_rose t=%0t", $time); end
  a_fell: assert property (@(posedge clk) disable iff (rst) $fell(q) |-> q2 && !$past(d))
    else begin fails++; $display("[FAIL] a_fell t=%0t", $time); end
  a_stable: assert property (@(posedge clk) disable iff (rst) $stable(q) == (q == q2))
    else begin fails++; $display("[FAIL] a_stable t=%0t", $time); end
  c_changed: cover property (@(posedge clk) disable iff (rst) $changed(q)) n++;
endmodule
"#);
    assert!(
        o.contains("[RESULT] changed=6 fails=0"),
        "sampled-value functions must read q's Preponed value; got: {}",
        o
    );
}

/// §16.5.1, §4.4.2.1: values are sampled once per time slot, in its Preponed
/// region. Here the property's clock rises two deltas into the slot, from the
/// process that the NBA update of `d` wakes, and `d` is still sampled with
/// its value from the start of the slot. The samples used to be retaken in
/// every delta the scheduler re-entered, which saw the new `d`. The
/// commercial simulator: 10 ticks, no failure.
#[test]
fn clock_in_a_later_delta_samples_slot_entry_values() {
    let o = out(r#"
module top;
  logic clk0 = 0, clk = 0, d = 0, e = 0;
  int fails = 0, n = 0;
  always #5 clk0 = ~clk0;
  always @(posedge clk0) d <= ~d;
  always @(negedge clk0) e = d;
  always @(d) begin #0 clk = 1; #1 clk = 0; end
  a_d: assert property (@(posedge clk) d == e)
    else begin fails++; $display("[FAIL] a_d t=%0t", $time); end
  c_n: cover property (@(posedge clk) 1) n++;
  initial #100 begin $display("[RESULT] ticks=%0d fails=%0d", n, fails); $finish; end
endmodule
"#);
    assert!(
        o.contains("[RESULT] ticks=10 fails=0"),
        "a tick in a later delta must use the slot's Preponed samples; got: {}",
        o
    );
}
