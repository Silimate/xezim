//! §7.2.1 / §26.3: a member select of a packed-struct package parameter,
//! read through an import inside an instantiated or bound module, selects
//! that member's bits, in a continuous assign, an `always_comb`, and an
//! initial block. The top module does not import the package.
#[test]
fn package_struct_parameter_member_in_submodule() {
    let result = xezim::simulate(
        r#"
package p;
  typedef struct packed {
    logic       a;
    logic       b;
    logic [4:0] c;
  } s_t;
  localparam s_t K = '{b: 1'b1, a: 1'b0, c: 5'd29};
endpackage
module probe;
  import p::*;
  logic [4:0] w, v;
  assign w = K.c;
  always_comb v = K.c;
  initial #1 $display("%m w=%b v=%b b=%b", w, v, K.b);
endmodule
module sub; endmodule
module t;
  sub u();
  probe direct();
endmodule
bind sub probe bound();
"#,
        100,
    )
    .unwrap();
    let mut lines: Vec<_> = result
        .output
        .iter()
        .map(|entry| entry.message.as_str())
        .collect();
    lines.sort();
    assert_eq!(
        lines,
        ["t.direct w=11101 v=11101 b=1", "t.u.bound w=11101 v=11101 b=1"]
    );
}
