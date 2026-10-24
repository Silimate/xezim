//! §14.13: a clocking input that is an unpacked array samples every element.
//! `ifc.cb.arr[i]` and `vif.cb.arr[i]` read element `i` of the sample. The
//! array has no scalar signal, so a sample stored under the array name was
//! empty and every element read 0.

use xezim::simulate;

const SRC: &str = r#"
`timescale 1ns/1ns
interface mon_if (input logic clk);
  logic [31:0] cnt [4];
  logic [7:0] flags;
  clocking cb @(posedge clk);
    input cnt, flags;
  endclocking
endinterface
class mon;
  virtual mon_if vif;
  function new(virtual mon_if v);
    vif = v;
  endfunction
  function int at1();
    return vif.cb.cnt[1];
  endfunction
endclass
module top;
  logic clk = 0;
  always #5 clk = ~clk;
  mon_if ifc (clk);
  mon m;
  initial begin
    m = new(ifc);
    ifc.cnt[1] = 32'h46e;
    ifc.cnt[2] = 32'h11;
    ifc.flags = 8'h04;
    @(posedge clk);
    #1;
    $display("cnt1=%h cnt2=%h vif=%h flag2=%b",
             ifc.cb.cnt[1], ifc.cb.cnt[2], m.at1(), ifc.cb.flags[2]);
    ifc.cnt[1] = 32'h99;
    ifc.flags = 8'h00;
    #1;
    $display("held=%h live=%h flag2=%b", ifc.cb.cnt[1], ifc.cnt[1], ifc.cb.flags[2]);
  end
endmodule
"#;

#[test]
fn clocking_unpacked_array_input_samples_each_element() {
    let sim = simulate(SRC, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(
        msgs,
        [
            "cnt1=0000046e cnt2=00000011 vif=0000046e flag2=1",
            "held=0000046e live=00000099 flag2=1",
        ]
    );
}
