//! §7.2, §7.4.1, §7.4.2: a member of an element of a packed array of
//! packed structs (`b_o.dw[0].value`, `s2.e[0].dw[1].value`) written inside
//! an instance below the top.
//!
//! Elaboration registers a packed struct variable's layout as a flat list of
//! member paths. The top module's declarations expanded a member that is a
//! packed array of structs into one entry per element (`dw[0]`,
//! `dw[0].value`, ...); the inlined-instance paths (ANSI and non-ANSI ports,
//! body variables) used another walker that registered only `dw` and an
//! element-0 alias `dw.value`. A write to `dw[i].value` on an output port, or
//! two array levels deep on a body variable, then found no layout: the
//! compiled store gave up and the interpreter wrote nothing, so the whole
//! variable stayed x. One walker now serves every declaration path.
//!
//! A packed-struct NET in an inlined instance had no layout at all, so a
//! member read (`n.a`) never woke its readers and a member write was lost.
//!
//! Expected values are Verilator's (5.046); the reference simulator prints
//! PASS for the first test as well.

use xezim::simulate;

fn lines(src: &str) -> Vec<String> {
    simulate(src, 100)
        .expect("simulate failed")
        .output
        .iter()
        .map(|o| o.message.clone())
        .collect()
}

fn assert_has(src: &str, expected: &[&str]) {
    let got = lines(src);
    for e in expected {
        assert!(got.iter().any(|l| l == e), "missing `{e}` in {got:#?}");
    }
}

/// Output ports written by NBA, `always_comb` and generate-loop continuous
/// assignments, and body variables one and two array levels deep, all with
/// package types.
#[test]
fn member_writes_through_array_elements_in_a_submodule() {
    assert_has(
        r#"
package p;
  typedef struct packed { logic [7:0] a; logic [7:0] b; } flat_t;
  typedef struct packed { logic [7:0] value; } dw_t;
  typedef struct packed { dw_t [1:0] dw; } nest_t;
  typedef struct packed { nest_t [1:0] e; } nest2_t;
endpackage
module m import p::*; (input logic clk, input logic [7:0] d,
                       output flat_t a_o, output nest_t b_o, output nest_t c_o,
                       output nest_t e_o);
  nest_t  loc;
  nest2_t s2;
  always_ff @(posedge clk) begin
    a_o.a <= d;
    a_o.b <= d + 1;
    b_o.dw[0].value <= d;
    b_o.dw[1].value <= d + 1;
    loc.dw[0].value <= d;
    loc.dw[1].value <= d + 1;
    s2.e[0].dw[0].value <= d;
    s2.e[0].dw[1].value <= d + 1;
    s2.e[1].dw[0].value <= d + 2;
    s2.e[1].dw[1].value <= d + 3;
  end
  for (genvar i = 0; i < 2; i++) begin : g_e
    assign e_o.dw[i].value = d + i;
  end
  always_comb begin
    c_o.dw[0].value = d;
    c_o.dw[1].value = d + 1;
  end
endmodule
module t;
  import p::*;
  logic clk = 0; logic [7:0] d; flat_t a; nest_t b, c, e;
  m u (.clk(clk), .d(d), .a_o(a), .b_o(b), .c_o(c), .e_o(e));
  initial begin
    d = 8'h10; #1 clk = 1; #1 clk = 0; #1;
    $display("a=%h b=%h c=%h e=%h u.loc=%h u.s2=%h", a, b, c, e, u.loc, u.s2);
    if (a === 16'h1011 && b === 16'h1110 && c === 16'h1110 && e === 16'h1110 &&
        u.loc === 16'h1110 && u.s2 === 32'h13121110)
      $display("PASS");
    else
      $display("FAIL");
    $finish;
  end
endmodule
"#,
        &[
            "a=1011 b=1110 c=1110 e=1110 u.loc=1110 u.s2=13121110",
            "PASS",
        ],
    );
}

/// Dynamic indices on both levels, and a read of an element member, with
/// typedefs local to the submodule.
#[test]
fn dynamic_indices_and_reads_with_local_typedefs() {
    assert_has(
        r#"
module m (input logic clk, input logic [7:0] d, output logic [31:0] o_flat, output logic [7:0] rd);
  typedef struct packed { logic [7:0] value; } dw_t;
  typedef struct packed { dw_t [1:0] dw; } nest_t;
  typedef struct packed { nest_t [1:0] e; } nest2_t;
  nest2_t s2;
  always_ff @(posedge clk)
    for (int i = 0; i < 2; i++)
      for (int j = 0; j < 2; j++)
        s2.e[i].dw[j].value <= d + 8'(2 * i + j);
  assign o_flat = s2;
  assign rd = s2.e[1].dw[0].value;
endmodule
module t;
  logic clk = 0; logic [7:0] d = 8'h20; logic [31:0] o; logic [7:0] rd;
  m u (.clk(clk), .d(d), .o_flat(o), .rd(rd));
  initial begin #1 clk = 1; #1 $display("p5: o=%h rd=%h", o, rd); end
endmodule
"#,
        &["p5: o=23222120 rd=22"],
    );
}

/// §7.2.1, §6.7.1: member reads of a packed-struct NET in a submodule wake
/// their readers (an `always_comb`, a continuous assignment, and a block
/// that also reads a bit of the whole net), for a net with a declaration
/// assignment and for one driven by a separate continuous assignment.
#[test]
fn struct_net_member_reads_in_a_submodule() {
    assert_has(
        r#"
module m;
  typedef struct packed { logic a; logic b; } t_t;
  t_t src;
  wire t_t n = src;
  wire t_t n2;
  assign n2 = src;
  logic x, y, z, x2;
  always_comb x = n.a;
  assign      y = n.a;
  always_comb begin
    z = n[0];
    if (n.a) z = 1'b1;
  end
  always_comb x2 = n2.a;
endmodule
module top;
  m u();
  initial begin
    u.src = 2'b00;
    #1 u.src = 2'b10;
    #1 $display("n1: x=%b y=%b z=%b x2=%b", u.x, u.y, u.z, u.x2);
  end
endmodule
"#,
        &["n1: x=1 y=1 z=1 x2=1"],
    );
}

/// Continuous assignments to members of a packed-struct net in a submodule.
#[test]
fn struct_net_member_writes_in_a_submodule() {
    assert_has(
        r#"
module m (input logic [3:0] d, output logic [3:0] o);
  typedef struct packed { logic [3:0] hi; logic [3:0] lo; } w_t;
  wire w_t n;
  assign n.lo = d;
  assign n.hi = ~d;
  assign o = n.hi;
endmodule
module top;
  logic [3:0] d = 4'h5; logic [3:0] o;
  m u(.d(d), .o(o));
  initial #1 $display("n2: o=%h n=%h", o, u.n);
endmodule
"#,
        &["n2: o=a n=a5"],
    );
}
