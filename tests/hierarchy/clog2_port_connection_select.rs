//! §11.5.1, §20.8.1, §23.3.3: a part-select bounded by `$clog2(...)` in a
//! port connection (`.raddr({a[$clog2(N)-1:3], 3'b000})`), read by a clocked
//! block of the child (issue #332).
//!
//! The child's reads of the port compile the connection expression. The
//! compiler did not fold `$clog2` of a constant, so the bounds counted as
//! dynamic, and the dynamic part-select used the labels as physical bit
//! positions: on `input [11:2] a` the select `a[11:3]` read bits 11..3 of a
//! 10-bit vector, two of them past its end. The clocked read saw x in the
//! upper bits while a continuous assignment, evaluated by the interpreter,
//! read the right value. `$clog2` of a constant now folds, and a bound that
//! still does not fold is mapped through the declared range (an ascending
//! range is left to the interpreter). This kept the VeeRwolf SoC's boot ROM
//! from returning instructions.
//!
//! Expected values are Verilator's (5.046).

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

/// The issue's testcase: a clocked and a continuous read of the port.
#[test]
fn clog2_bound_in_a_port_connection() {
    assert_has(
        r#"
module leaf (input wire clk, input wire [11:0] raddr, output reg [11:0] q, output wire [11:0] qa);
  always @(posedge clk) q <= raddr;
  assign qa = raddr;
endmodule
module wrap #(parameter N = 4096) (input wire clk, input wire [11:2] a, output wire [11:0] q, output wire [11:0] qa);
  wire [11:0] direct = {a[$clog2(N)-1:3], 3'b000};
  leaf r (.clk(clk), .raddr({a[$clog2(N)-1:3], 3'b000}), .q(q), .qa(qa));
  initial #2 $display("k7: direct=%h $bits=%0d", direct, $bits({a[$clog2(N)-1:3], 3'b000}));
endmodule
module tb;
  reg clk = 0; wire [11:0] q, qa;
  wrap w (.clk(clk), .a(10'h3ff), .q(q), .qa(qa));
  initial begin #1 clk = 1; #1 $display("k7: q=%h qa=%h", q, qa); end
endmodule
"#,
        &["k7: direct=ff8 $bits=12", "k7: q=ff8 qa=ff8"],
    );
}

/// The VeeRwolf boot ROM shape: the memory index is a part-select of the
/// connected port, `mem[raddr[AW-1:3]]`, read on the clock.
#[test]
fn boot_rom_read_through_the_port() {
    assert_has(
        r#"
module dpram64 #(parameter SIZE = 0) (input wire clk, input wire [$clog2(SIZE)-1:0] raddr, output reg [63:0] dout);
  localparam AW = $clog2(SIZE);
  reg [63:0] mem [0:SIZE/8-1];
  always @(posedge clk) dout <= mem[raddr[AW-1:3]];
endmodule
module wrap #(parameter MEM_SIZE = 0) (input wire clk, input wire [$clog2(MEM_SIZE)-1:2] i_wb_adr, output wire [63:0] q);
  dpram64 #(.SIZE(MEM_SIZE)) ram (.clk(clk), .raddr({i_wb_adr[$clog2(MEM_SIZE)-1:3],3'b000}), .dout(q));
endmodule
module tb;
  reg clk = 0; wire [63:0] q; reg [31:0] adr = 32'h4;
  wrap #(.MEM_SIZE(32'h1000)) bootrom (.clk(clk), .i_wb_adr(adr[11:2]), .q(q));
  initial begin
    bootrom.ram.mem[0] = 64'h7c029073aaaa02b7;
    #1 clk = 1; #1 $display("h2: q=%h raddr=%h", q, bootrom.ram.raddr);
  end
endmodule
"#,
        &["h2: q=7c029073aaaa02b7 raddr=000"],
    );
}

/// Bounds that do not fold at compile time (a constant function), on a
/// descending range that does not start at 0 and on an ascending one, and a
/// `$clog2` base of an indexed part-select.
#[test]
fn unfolded_bounds_map_through_the_declared_range() {
    assert_has(
        r#"
module leaf (input wire clk, input wire [11:2] a, input wire [2:11] b,
             output reg [8:0] qa, output reg [8:0] qb, output reg [3:0] qc);
  function automatic int f(int x); return x + 7; endfunction
  always @(posedge clk) begin
    qa <= a[f(4):3];
    qb <= b[3:f(4)];
    qc <= a[$clog2(4096)-1 -: 4];
  end
endmodule
module tb;
  reg clk = 0; wire [8:0] qa, qb; wire [3:0] qc;
  leaf u (.clk(clk), .a(10'b1010011101), .b(10'b1100101011), .qa(qa), .qb(qb), .qc(qc));
  initial begin #1 clk = 1; #1 $display("j1: qa=%b qb=%b qc=%b", qa, qb, qc); end
endmodule
"#,
        &["j1: qa=101001110 qb=100101011 qc=1010"],
    );
}

/// A `$clog2` argument wider than 32 bits keeps its value in the fold.
#[test]
fn wide_clog2_argument() {
    assert_has(
        r#"
// a $clog2 argument wider than 32 bits in a clocked select bound
module leaf (input wire clk, input logic [31:0] a, output logic [31:0] q);
  always @(posedge clk) q <= a[$clog2(64'h1_0000_0000)-1:0];
endmodule
module tb;
  reg clk = 0; logic [31:0] q;
  leaf u (.clk(clk), .a(32'hDEADBEEF), .q(q));
  initial begin #1 clk = 1; #1 $display("c3w: q=%h", q); end
endmodule
"#,
        &["c3w: q=deadbeef"],
    );
}
