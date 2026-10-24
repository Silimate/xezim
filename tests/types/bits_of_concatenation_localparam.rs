//! §20.6.2 / §11.4.12: `$bits({a, b, ...})` is the sum of the operand
//! widths. In an instantiated module's body localparam it read 0. Operands
//! are ports, a struct port, a package typedef, a local variable and a sized
//! literal.

use xezim::simulate;

const SRC: &str = r#"
package pk;
  typedef struct packed { logic [31:0] a; logic [31:0] b; } cd_t;
  typedef logic [3:0] mubi_t;
endpackage
module m import pk::*; #(parameter int unsigned W = 8) (
  input  logic [31:0] addr_i,
  input  logic        req_i,
  input  logic [W-1:0] data_i,
  output cd_t         crash_o,
  input  mubi_t       en_i
);
  logic [3:0] busy_d;
  localparam int N1 = $bits({addr_i});
  localparam int N3 = $bits({addr_i, req_i, data_i});
  localparam int N5 = $bits({addr_i, req_i, data_i, crash_o, en_i});
  localparam int N6 = $bits({addr_i, req_i, data_i, crash_o, en_i, busy_d, 1'b0});
  initial $display("%0d %0d %0d %0d", N1, N3, N5, N6);
endmodule
module top; cd_t c; m u(.addr_i(32'h0), .req_i(1'b0), .data_i(8'h0), .crash_o(c), .en_i(4'h5)); endmodule
"#;

#[test]
fn bits_of_concatenation_localparam() {
    let sim = simulate(SRC, 10).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(msgs, ["32 41 109 114",],);
}
