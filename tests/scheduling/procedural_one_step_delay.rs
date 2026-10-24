//! IEEE 1800-2017 §A.2.2.3 / §3.14.3: `1step` is a `delay_value`, so
//! `#1step;` is a procedural delay of one global time precision step. It
//! used to parse as `#1` followed by an undeclared identifier `step`.
use xezim::simulate;

fn t_lines(sim: &xezim::compiler::Simulator) -> Vec<String> {
    sim.output
        .iter()
        .map(|o| o.message.trim_end().to_string())
        .filter(|l| l.starts_with("T|"))
        .collect()
}

/// One step is 1ps under `timescale 1ns/1ps`, in a module, a class task, a
/// package class task, and a fork branch; a forced net's combinational
/// fanout has settled after it.
#[test]
fn procedural_one_step_delay() {
    const SRC: &str = r#"
`timescale 1ns/1ps
package pk;
  class pc;
    task t(output realtime r); #1step; r = $realtime; endtask
  endclass
endpackage
class c;
  task t(output realtime r); #1step; r = $realtime; endtask
endclass
module top;
  realtime a, b, d, e;
  logic x = 0;
  wire y = ~x;
  c o = new;
  pk::pc po = new;
  initial begin
    #5;
    #1step;
    a = $realtime;
    o.t(b);
    po.t(d);
    fork
      #1step;
      #2;
    join_any
    e = $realtime;
    force x = 1;
    #1step;
    $display("T|a|%.3f", a);
    $display("T|b|%.3f", b);
    $display("T|d|%.3f", d);
    $display("T|e|%.3f", e);
    $display("T|y|%b", y);
  end
endmodule
"#;
    let sim = simulate(SRC, 100_000).expect("simulate");
    assert_eq!(
        t_lines(&sim),
        ["T|a|5.001", "T|b|5.002", "T|d|5.003", "T|e|5.004", "T|y|0"]
    );
}
