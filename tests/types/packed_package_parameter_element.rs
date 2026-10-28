//! IEEE 1800-2017 §7.4.5, §26.3: indexing a package parameter of a packed
//! multi-dimensional type selects an element, through the scoped name or an
//! imported name, in procedural code, a continuous assign, an instantiated
//! module and a package function. It selected one bit of the flat vector.

use xezim::simulate;

const SRC: &str = r#"
package mp;
  parameter logic [3:0][3:0] Q = {4'h4, 4'h3, 4'h2, 4'h1};
  localparam logic [3:0][3:0] LQ = {4'h8, 4'h7, 4'h6, 4'h5};
  function automatic logic [3:0] f(logic [1:0] i); return Q[i]; endfunction
endpackage
module sub import mp::*; (input logic [1:0] n, output logic [3:0] a, output logic [3:0] c);
  assign a = mp::Q[n];
  always_comb c = LQ[n];
endmodule
module top;
  import mp::*;
  logic [1:0] n;
  logic [3:0] b, a, c;
  assign b = mp::LQ[n];
  sub u(.n(n), .a(a), .c(c));
  initial begin
    for (int i = 0; i < 4; i++) begin
      n = i[1:0];
      #1 $display("%h %h %h %h %h %h %h %h", mp::Q[n], mp::LQ[n], Q[n], LQ[n], b, a, c, f(n));
    end
    $display("%h %h", mp::Q[2], LQ[3]);
  end
endmodule
"#;

#[test]
fn packed_package_parameter_element() {
    let sim = simulate(SRC, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(
        msgs,
        [
            "1 5 1 5 5 1 5 1",
            "2 6 2 6 6 2 6 2",
            "3 7 3 7 7 3 7 3",
            "4 8 4 8 8 4 8 4",
            "3 8",
        ],
    );
}
