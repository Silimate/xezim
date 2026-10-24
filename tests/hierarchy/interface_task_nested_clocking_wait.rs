//! IEEE 1800-2017 §14.10: `@cb` inside an interface task that another task
//! of the same interface calls waits for the clocking block's clock edge.
//! The block resolved only from the innermost task frame, so a nested call
//! (`outer` -> `wait_clks` -> `repeat (n) @cb`) found no block and returned
//! at once.

use xezim::simulate;

const SRC: &str = r#"
`timescale 1ns/1ps
interface ci(inout wire clk);
  clocking cb @(posedge clk); endclocking
  task automatic wait_clks(int n); repeat (n) @cb; endtask
  task automatic outer(int n); #1ns; wait_clks(n); endtask
  logic o_clk = 0;
  assign clk = o_clk;
  initial forever begin #5 o_clk = 1; #5 o_clk = 0; end
endinterface
module top;
  wire clk; ci i(.clk(clk));
  initial begin
    i.wait_clks(3);  $display("A %0d", $time);
    i.outer(3);      $display("B %0d", $time);
    $finish;
  end
endmodule
"#;

#[test]
fn interface_task_nested_clocking_wait() {
    let sim = simulate(SRC, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(msgs, ["A 25", "B 55",],);
}
