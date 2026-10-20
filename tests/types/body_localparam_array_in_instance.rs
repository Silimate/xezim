//! §6.20.2: an unpacked-array localparam declared in the body of an
//! instantiated module keeps its elements. Only the top module's body went
//! through array registration, so inside an instance every `C[i]` read 0
//! (prim_lfsr's polynomial table, and its CoeffCheck_A assertion).

use xezim::simulate;

const SRC: &str = r#"
module m #(parameter int unsigned W = 5);
  localparam int unsigned OFF = 3;
  localparam logic [15:0] C [6] = '{16'h6, 16'hC, 16'h14, 16'h30, 16'h60, 16'hB8};
  logic [W-1:0] coeffs;
  assign coeffs = C[W-OFF][W-1:0];
  initial begin
    #1;
    $display("elem=%h coeffs=%h msb=%b", C[W-OFF], coeffs, coeffs[W-1]);
  end
endmodule
module top; m #(.W(5)) u(); initial #2 $finish; endmodule
"#;

#[test]
fn body_localparam_array_in_instance() {
    let sim = simulate(SRC, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(msgs, ["elem=0014 coeffs=14 msb=1",],);
}
