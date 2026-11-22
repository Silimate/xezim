//! §9.4.2.2: `@*` adds every net and variable read by the statement it
//! guards, and that statement may itself start with a delay or wait:
//! `always @(*) #10 clk <= ~clk;` is a clock generator. The read collector
//! skipped any statement behind a procedural timing control, so the inferred
//! list came out empty and the block went to the comb-settle path, which
//! drops the event control: it re-ran `clk <= ~clk` every delta and stalled
//! at the first edge. Its own written variables were also dropped from the
//! list, but a block that suspends is only waiting at `@*` between
//! activations: the nonblocking update lands after it re-arms and wakes it,
//! while a blocking write made as it runs wakes nothing.
//!
//! Every generator is started by an x -> 0 assignment at time 5, which is an
//! event in any language mode (a declaration initializer is not, §6.8).
//! Expected values agree with the reference simulator and Icarus Verilog.

fn output(source: &str) -> String {
    let sim = xezim::simulate(source, 5000).expect("simulate");
    sim.output
        .iter()
        .map(|line| line.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_passes(result: &str, checks: usize) {
    assert!(!result.contains("FAIL"), "{result}");
    assert_eq!(
        result.lines().filter(|l| l.starts_with("PASS")).count(),
        checks,
        "{result}"
    );
}

const CHECK_TASK: &str = r#"
  task check(input string name, input integer got, input integer want);
    if (got == want) $display("PASS %s=%0d", name, got);
    else $display("FAIL %s=%0d want %0d", name, got, want);
  endtask
"#;

/// The reported clock generator, in its three spellings: posedges at 15,
/// 35, ..., 1995.
#[test]
fn star_delay_nba_clock_keeps_toggling() {
    let source = r#"
`timescale 1ns/1ns
module t;
  reg c1, c2, c3;
  integer n1 = 0, n2 = 0, n3 = 0;
  initial #5 begin c1 = 0; c2 = 0; c3 = 0; end
  always @(*) #10 c1 <= ~c1;
  always @* #10 c2 <= ~c2;
  always @(*) begin #10 c3 <= ~c3; end
  always @(posedge c1) n1++;
  always @(posedge c2) n2++;
  always @(posedge c3) n3++;
  CHECK_TASK
  initial begin
    #2000;
    check("n1", n1, 100);
    check("n2", n2, 100);
    check("n3", n3, 100);
    $finish;
  end
endmodule
"#
    .replace("CHECK_TASK", CHECK_TASK);
    assert_passes(&output(&source), 3);
}

/// Only a change while the block waits at `@*` wakes it: a blocking write
/// made as it runs does not, nor does a nonblocking update that lands during
/// a later delay of the same activation. Each toggles once.
#[test]
fn star_delay_wakes_only_while_waiting() {
    let source = r#"
`timescale 1ns/1ns
module t;
  reg b1, b2, d1;
  integer nb1 = 0, nb2 = 0, nd1 = 0;
  initial #5 begin b1 = 0; b2 = 0; d1 = 0; end
  always @(*) #10 b1 = ~b1;
  always @* begin #10 b2 = ~b2; end
  always @* begin #5; d1 <= ~d1; #5; end
  always @(posedge b1) nb1++;
  always @(posedge b2) nb2++;
  always @(posedge d1) nd1++;
  CHECK_TASK
  initial begin
    #2000;
    check("nb1", nb1, 1);
    check("nb2", nb2, 1);
    check("nd1", nd1, 1);
    $finish;
  end
endmodule
"#
    .replace("CHECK_TASK", CHECK_TASK);
    assert_passes(&output(&source), 3);
}

/// A delayed and a wait-guarded follower of `a` track it; a body that reads
/// nothing never wakes, so `x` stays x.
#[test]
fn star_delay_and_wait_bodies_follow_their_reads() {
    let source = r#"
`timescale 1ns/1ns
module t;
  reg a = 0, en = 1;
  reg y, w, x;
  always #10 a = ~a;
  always @* #3 y = a;
  always @* wait (en) w = a;
  always @* #5 x = 1'b1;
  CHECK_TASK
  initial begin
    #16;
    check("y_t16", y, 1);
    check("w_t16", w, 1);
    #10;
    check("y_t26", y, 0);
    check("w_t26", w, 0);
    #74;
    if (x === 1'bx) $display("PASS x"); else $display("FAIL x=%b", x);
    $finish;
  end
endmodule
"#
    .replace("CHECK_TASK", CHECK_TASK);
    assert_passes(&output(&source), 5);
}
