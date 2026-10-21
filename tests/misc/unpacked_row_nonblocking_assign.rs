//! A nonblocking assignment to a ROW of a multi-dimensional unpacked array.
//!
//! Each element of `q[2][2]` is its own signal, so `q[1] <= ic` was queued
//! against a name with no storage and the row read back X (or the old value).
//! Ibex's lockstep input delay queue is exactly this shape, and its shadow
//! core then saw all-zero icache tag data and raised an alert.

use std::process::Command;

fn run(tag: &str, src: &str) -> String {
    let dir = std::env::temp_dir().join(format!("xezim_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.sv");
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--simulate", "-s", "tb", path.to_str().unwrap(), "--no-cache"])
        .output()
        .expect("run xezim");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn row_nonblocking_assign_updates_each_element() {
    let text = run(
        "rownba",
        r#"module tb;
  logic clk = 0;
  logic [3:0] in_i [2];
  logic [3:0] a [2][2], b [2][2], c [2][2], e [2][2], f [2][2];
  always #5 clk = ~clk;
  always @(posedge clk) begin
    a[0] <= '{default: 0};
    b[1] <= in_i;
    c[0] <= '{4'h1, 4'h2};
    e[1] <= '{4'h3, 4'h4};
    f[0][1] <= 4'h7;
  end
  initial begin
    in_i[0] = 4'h3; in_i[1] = 4'h5;
    #6;
    if ({a[0][0], a[0][1]} !== 8'h00) $display("FAIL a=%h%h", a[0][0], a[0][1]);
    if ({b[1][0], b[1][1]} !== 8'h35) $display("FAIL b=%h%h", b[1][0], b[1][1]);
    if ({c[0][0], c[0][1]} !== 8'h12) $display("FAIL c=%h%h", c[0][0], c[0][1]);
    if ({e[1][0], e[1][1]} !== 8'h34) $display("FAIL e=%h%h", e[1][0], e[1][1]);
    if (f[0][1] !== 4'h7) $display("FAIL f=%h", f[0][1]);
    $display("DONE");
    $finish;
  end
endmodule
"#,
    );
    assert!(text.contains("DONE"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
}

/// A two-stage delay line built from row assignments, read back through a
/// port connected to a row, as `ibex_lockstep` does.
#[test]
fn row_nonblocking_delay_line_through_row_port() {
    let text = run(
        "rowline",
        r#"module shadow(input logic [3:0] p [2], output logic [3:0] s0, output logic [3:0] s1);
  assign s0 = p[0]; assign s1 = p[1];
endmodule
module tb;
  logic clk = 0; logic rst_n = 0;
  always #5 clk = ~clk;
  logic [3:0] ic [2];
  logic [3:0] q [2][2];
  logic [3:0] s0, s1;
  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) begin
      for (int i = 0; i < 2; i++) q[i] <= '{default: 0};
    end else begin
      for (int i = 0; i < 1; i++) q[i] <= q[i+1];
      q[1] <= ic;
    end
  end
  shadow u (.p(q[0]), .s0(s0), .s1(s1));
  initial begin
    ic[0] = 4'h3; ic[1] = 4'h5;
    #12 rst_n = 1;
    repeat (3) begin
      @(posedge clk); #1;
      ic[0] = ic[0] + 1; ic[1] = ic[1] + 1;
    end
    // `ic` is 3,5 at the first edge after reset and counts up each edge; after
    // the fourth edge the read side holds the value from two edges earlier.
    @(posedge clk); #1;
    if ({s0, s1} !== 8'h57) $display("FAIL s=%h%h", s0, s1);
    $display("DONE");
    $finish;
  end
endmodule
"#,
    );
    assert!(text.contains("DONE"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
}
