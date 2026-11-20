//! IEEE 1800-2023 §4.4.2, §4.5: the NBA region comes once the Active region
//! is empty, and the processes a clock edge wakes run in that Active region.
//! A process that resumes from a delay at the time of a clock edge and
//! writes with `<=` (a testbench's `#10 d <= 1;` landing on an edge) must not
//! be seen by that edge's flops and `@(posedge clk)` waiters: they read `d`
//! before the slot's NBAs commit.
//!
//! xezim committed the slot's NBAs first, so the flop captured the new `d`
//! one cycle early and the waiter printed it.

use xezim::simulate;

const SRC: &str = r#"
module top;
  logic clk = 0, d = 0, q;
  initial forever #5 clk = ~clk;
  always @(posedge clk) q <= d;
  initial begin
    @(posedge clk);
    #10 d <= 1;
  end
  initial begin
    @(posedge clk);
    forever @(posedge clk) $display("T|%0t|d=%b q=%b", $time, d, q);
  end
  initial #40 $finish;
endmodule
"#;

#[test]
fn clock_edge_processes_run_before_the_slots_nbas() {
    let sim = simulate(SRC, 1000).expect("simulate failed");
    let got: Vec<String> = sim
        .output
        .iter()
        .map(|o| o.message.trim_end().to_string())
        .filter(|l| l.starts_with("T|"))
        .collect();
    assert_eq!(got, ["T|15|d=0 q=0", "T|25|d=1 q=0", "T|35|d=1 q=1"]);
}
