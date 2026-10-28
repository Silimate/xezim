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

/// §14.4 `input #3`: an element change after the sample time is not sampled.
#[test]
fn clocking_unpacked_array_input_keeps_its_skew() {
    let src = r#"
`timescale 1ns/1ns
module top;
  logic clk = 0;
  logic [3:0] arr [2];
  always #5 clk = ~clk;
  clocking cb @(posedge clk); input #3 arr; endclocking
  initial begin
    arr[1] = 4'h5; #4 arr[1] = 4'h9;
    #2 $display("arr1=%h", cb.arr[1]);
  end
endmodule
"#;
    let sim = simulate(src, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(msgs, ["arr1=5"]);
}

/// Every element is sampled, however large the array. An element written at
/// the edge keeps its pre-edge value in the sample.
#[test]
fn clocking_unpacked_array_input_samples_a_large_array() {
    let src = r#"
`timescale 1ns/1ns
interface big_if (input logic clk);
  logic [7:0] mem [5000];
  clocking cb @(posedge clk); input mem; endclocking
endinterface
module top;
  logic clk = 0;
  always #5 clk = ~clk;
  big_if ifc (clk);
  logic [7:0] arr [5000];
  clocking cb @(posedge clk); input arr; endclocking
  initial begin
    arr[4999] = 8'h22; ifc.mem[4999] = 8'h44;
    @(posedge clk);
    arr[4999] = 8'hbb; ifc.mem[4999] = 8'hcc;
    #1 $display("arr=%h mem=%h", cb.arr[4999], ifc.cb.mem[4999]);
  end
endmodule
"#;
    let sim = simulate(src, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(msgs, ["arr=22 mem=44"]);
}
