//! IEEE 1800-2023 §6.21, §12.7.1: a variable declared in a for-init is
//! automatic and local to the loop. In a task whose own (static) variable
//! has the same name, the loop's reads and writes must reach the loop
//! variable. They reached the task's static cell instead: the condition read
//! the static (x, never assigned), and the loop ran no iteration, with and
//! without a timing control in its body. Verilator 5.036 (`--binary
//! --timing`) prints the expected lines.

use xezim::simulate;

const SRC: &str = r#"
module top;
  logic clk = 0;
  always #5 clk = ~clk;
  task m(input integer n);
    integer j;
    begin
      for (integer j = 0; j < n; j++) begin
        $display("T|static|j=%0d", j);
      end
    end
  endtask
  task m_timed(input integer n);
    integer j;
    begin
      @(posedge clk);
      for (integer j = 0; j < n; j++) begin
        @(posedge clk);
        $display("T|timed|j=%0d|%0t", j, $time);
      end
    end
  endtask
  // The task's own `j` keeps its value across the loop.
  task m_keep(input integer n);
    integer j;
    begin
      j = 7;
      for (integer j = 0; j < n; j++) $display("T|keep|j=%0d", j);
      $display("T|keep|after=%0d", j);
    end
  endtask
  initial begin
    m(2);
    m_timed(2);
    m_keep(2);
    $display("T|done|%0t", $time);
    $finish;
  end
endmodule
"#;

#[test]
fn for_init_variable_hides_a_static_task_variable() {
    let sim = simulate(SRC, 1000).expect("simulate failed");
    let got: Vec<String> = sim
        .output
        .iter()
        .map(|o| o.message.trim_end().to_string())
        .filter(|l| l.starts_with("T|"))
        .collect();
    assert_eq!(
        got,
        [
            "T|static|j=0",
            "T|static|j=1",
            "T|timed|j=0|15",
            "T|timed|j=1|25",
            "T|keep|j=0",
            "T|keep|j=1",
            "T|keep|after=7",
            "T|done|25",
        ]
    );
}
