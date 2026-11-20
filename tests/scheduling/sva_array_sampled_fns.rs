//! IEEE 1800-2023 §16.9.3: the operand of `$past`, `$stable` and `$changed`
//! may be a whole unpacked array, whose sampled value is all its cells. The
//! sampled-value watch read the array's name, which is no signal, so its
//! history never changed: `$stable(m)` always held, `$changed(m)` never
//! did, and `m == $past(m)` compared the array with that constant.
//!
//! The array is written on the falling edge only every other cycle, so at
//! a rising edge it changed since the last one exactly when `we` was set at
//! that one. Verilator 5.036 does not accept a whole unpacked array as the
//! operand of these functions.

use xezim::simulate;

const SRC: &str = r#"
module top;
  logic clk = 0;
  always #1 clk = ~clk;
  logic [3:0] m [2];
  logic [3:0] d [3:0];
  logic [3:0] din = 1;
  bit we = 0;
  int n = 0, f_chg = 0, f_stab = 0, f_past = 0, f_desc = 0;
  initial begin
    m[0] = 0; m[1] = 0;
    foreach (d[i]) d[i] = 0;
  end
  always @(negedge clk) begin
    if (we) begin
      m[din[0]] <= din;
      d[din[1:0]] <= din;
    end
    din <= din + 1;
    we <= !we;
  end
  a_chg:  assert property (@(posedge clk) $time < 4 || $changed(m) == $past(we)) else f_chg = f_chg + 1;
  a_stab: assert property (@(posedge clk) $time < 4 || $stable(m) == !$past(we)) else f_stab = f_stab + 1;
  a_past: assert property (@(posedge clk) $time < 4 || (m == $past(m)) == !$past(we)) else f_past = f_past + 1;
  a_desc: assert property (@(posedge clk) $time < 4 || (d == $past(d)) == !$past(we)) else f_desc = f_desc + 1;
  always @(posedge clk) n = n + 1;
  initial begin
    #20 $display("RESULT edges=%0d changed=%0d stable=%0d past=%0d desc=%0d", n, f_chg, f_stab, f_past, f_desc);
    $finish;
  end
endmodule
"#;

#[test]
fn sampled_value_functions_of_a_whole_unpacked_array() {
    let sim = simulate(SRC, 1000).expect("simulate failed");
    let line = sim
        .output
        .iter()
        .map(|o| o.message.trim_end().to_string())
        .find(|l| l.starts_with("RESULT"))
        .unwrap_or_default();
    assert_eq!(line, "RESULT edges=10 changed=0 stable=0 past=0 desc=0");
}
