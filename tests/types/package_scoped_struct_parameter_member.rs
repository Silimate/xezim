//! §7.2.1 / §26.3: `p::K.c`, a member select of a packed-struct package
//! parameter named with the package scope, selects that member of the
//! package's own `K`, with no import, in a continuous assign, a procedural
//! read, a function, and a submodule, and while the top module declares its
//! own `K`.
#[test]
fn package_scoped_struct_parameter_member() {
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
module sub;
  logic [4:0] w;
  assign w = p::K.c;
  initial #1 $display("sub w=%b b=%b", w, p::K.b);
endmodule
module t;
  logic [6:0] K;
  logic [4:0] w;
  logic       b;
  assign w = p::K.c;
  assign b = p::K.b;
  sub u();
  function automatic logic [4:0] f();
    return p::K.c;
  endfunction
  initial begin
    logic [4:0] x;
    K = 7'h03;
    x = p::K.c;
    #2 $display("top w=%b b=%b x=%b f=%b", w, b, x, f());
  end
endmodule
"#,
        100,
    )
    .unwrap();
    let lines: Vec<_> = result
        .output
        .iter()
        .map(|entry| entry.message.as_str())
        .collect();
    assert_eq!(
        lines,
        ["sub w=11101 b=1", "top w=11101 b=1 x=11101 f=11101"]
    );
}
