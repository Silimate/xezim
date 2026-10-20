//! §6.20.2: an unpacked-array package parameter keeps its elements. It was
//! evaluated as one scalar at the element width, so `pkg::A[i]` read 0 and a
//! module parameter defaulted or overridden from it lost its layout (Ibex's
//! PMPRstAddr chain: every ASSERT_INIT on it failed at time 0). Covers the
//! scoped default, an imported dimension name, and a chain of `.R(R)` hops.

use xezim::simulate;

const SRC: &str = r#"
package pk;
  parameter int unsigned MAXR = 4;
  parameter int unsigned MSB = 33;
  parameter logic [MSB:0] AddrRst [MAXR] = '{34'h1, 34'h2, 34'h3, 34'h4};
endpackage
module leaf import pk::*; #(parameter int unsigned G = 0, parameter int unsigned N = 4,
              parameter logic [MSB:0] R [MAXR] = pk::AddrRst);
  localparam int unsigned W = (G > 0) ? MSB - G : 32;
  for (genvar i = 0; i < N; i++) begin : g
    initial $display("leaf%0d word=%h lo=%b", i, R[i], R[i][MSB-W:0]);
  end
endmodule
module mid import pk::*; #(parameter int unsigned N = 4, parameter logic [MSB:0] R [MAXR] = pk::AddrRst);
  leaf #(.N(N), .R(R)) u();
endmodule
module top import pk::*; #(parameter int unsigned N = 2, parameter logic [MSB:0] R [MAXR] = pk::AddrRst);
  mid #(.N(N), .R(R)) u();
  initial #1 $finish;
endmodule
"#;

#[test]
fn package_unpacked_array_parameter() {
    let sim = simulate(SRC, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(
        msgs,
        ["leaf0 word=000000001 lo=01", "leaf1 word=000000002 lo=10"],
    );
}
