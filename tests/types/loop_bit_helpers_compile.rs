//! Helper functions that loop over a function-level `integer` with
//! `i = i + 1` (or `i += 3`, `i = i - 1`) and store single bits at that
//! signed index (`v[i] = ...`) must compile when called from a clocked
//! block, and the compiled body must match the interpreter bit for bit.
//!
//! Two gaps kept them interpreted:
//! - the purity walker had no arm for the `AssignExpr` a `for` step parses
//!   to, so the loop branded its function impure;
//! - a single-bit store into a register-backed local took only an unsigned
//!   index, so an `integer` index bailed (`blocking_frame_target`).
//!
//! Each function has an impure twin (it also bumps a module counter, a side
//! effect that must keep it on the interpreter). Every cycle the clocked
//! block computes both, and the testbench compares them with `!==`, over
//! inputs with x and z bits, an x/z index, negative and out-of-range indices,
//! loop bounds of 0 and 1, and steps of +2, -1 and +3. The known inputs'
//! values are also checked against hand-derived results, so both paths
//! can't be wrong the same way. `XEZIM_PROBE_INLINE` pins what compiled, and
//! `XEZIM_COMPILE_METHODS=0` keeps the twins off the method-compile tier, so
//! they run on the interpreter itself.

use std::process::Command;

const SRC: &str = r#"
`timescale 1ns/1ns
module top;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [7:0] s = 0;
  logic [3:0] k = 0;
  int calls = 0, edges = 0, mism = 0;

  // Ripple-carry +1 and -1, the counter helper shape.
  function [7:0] incr(input [7:0] s1, input [3:0] k1);
    reg [7:0] v, t;
    integer i;
    begin
      t = 8'b1;
      v = s1;
      for (i = 1; i < 8; i = i + 1) t[i] = v[i-1] & t[i-1];
      for (i = 0; i < 8; i = i + 1) if (t[i] == 1'b1) v[i] = ~v[i];
      incr = v;
    end
  endfunction
  function [7:0] incr_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v, t;
    integer i;
    begin
      calls = calls + 1;
      t = 8'b1;
      v = s1;
      for (i = 1; i < 8; i = i + 1) t[i] = v[i-1] & t[i-1];
      for (i = 0; i < 8; i = i + 1) if (t[i] == 1'b1) v[i] = ~v[i];
      incr_i = v;
    end
  endfunction
  function automatic [7:0] decr(input [7:0] s1, input [3:0] k1);
    reg [7:0] v, t;
    integer i;
    begin
      t = 8'b0;
      v = s1;
      for (i = 1; i < 8; i = i + 1) t[i] = v[i-1] | t[i-1];
      for (i = 0; i < 8; i = i + 1) if (t[i] == 1'b0) v[i] = ~v[i];
      decr = v;
    end
  endfunction
  function automatic [7:0] decr_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v, t;
    integer i;
    begin
      calls = calls + 1;
      t = 8'b0;
      v = s1;
      for (i = 1; i < 8; i = i + 1) t[i] = v[i-1] | t[i-1];
      for (i = 0; i < 8; i = i + 1) if (t[i] == 1'b0) v[i] = ~v[i];
      decr_i = v;
    end
  endfunction
  // Indices -2, -1, 8 and 9 are out of range: their stores write nothing.
  function [7:0] flip(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      v = s1;
      for (i = -2; i < 10; i = i + 1) v[i] = ~v[i];
      flip = v;
    end
  endfunction
  function [7:0] flip_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      calls = calls + 1;
      v = s1;
      for (i = -2; i < 10; i = i + 1) v[i] = ~v[i];
      flip_i = v;
    end
  endfunction
  // v[-1] = v[0] writes nothing; v[7] = v[8] reads out of range: x.
  function [7:0] shr(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      v = s1;
      for (i = 0; i < 9; i = i + 1) v[i-1] = v[i];
      shr = v;
    end
  endfunction
  function [7:0] shr_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      calls = calls + 1;
      v = s1;
      for (i = 0; i < 9; i = i + 1) v[i-1] = v[i];
      shr_i = v;
    end
  endfunction
  function [7:0] none(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      v = s1;
      for (i = 0; i < 0; i = i + 1) v[i] = 1'b0;
      none = v;
    end
  endfunction
  function [7:0] none_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      calls = calls + 1;
      v = s1;
      for (i = 0; i < 0; i = i + 1) v[i] = 1'b0;
      none_i = v;
    end
  endfunction
  function [7:0] one(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      v = s1;
      for (i = 0; i < 1; i = i + 1) v[i] = 1'b0;
      one = v;
    end
  endfunction
  function [7:0] one_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      calls = calls + 1;
      v = s1;
      for (i = 0; i < 1; i = i + 1) v[i] = 1'b0;
      one_i = v;
    end
  endfunction
  function [7:0] even(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      v = s1;
      for (i = 0; i < 8; i = i + 2) v[i] = 1'b0;
      even = v;
    end
  endfunction
  function [7:0] even_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      calls = calls + 1;
      v = s1;
      for (i = 0; i < 8; i = i + 2) v[i] = 1'b0;
      even_i = v;
    end
  endfunction
  function [7:0] rev(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      v = s1;
      for (i = 7; i >= 0; i = i - 1) v[i] = s1[7 - i];
      rev = v;
    end
  endfunction
  function [7:0] rev_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      calls = calls + 1;
      v = s1;
      for (i = 7; i >= 0; i = i - 1) v[i] = s1[7 - i];
      rev_i = v;
    end
  endfunction
  function [7:0] every3(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      v = s1;
      for (i = 1; i < 8; i += 3) v[i] = 1'b1;
      every3 = v;
    end
  endfunction
  function [7:0] every3_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer i;
    begin
      calls = calls + 1;
      v = s1;
      for (i = 1; i < 8; i += 3) v[i] = 1'b1;
      every3_i = v;
    end
  endfunction
  // A signed index from an input: k1 - 2 is negative for k1 < 2, past the
  // top for k1 > 9, and x when k1 has an x or z bit; none of those writes.
  function [7:0] at_k(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer j;
    begin
      v = s1;
      j = k1;
      j = j - 2;
      v[j] = ~v[j];
      at_k = v;
    end
  endfunction
  function [7:0] at_k_i(input [7:0] s1, input [3:0] k1);
    reg [7:0] v;
    integer j;
    begin
      calls = calls + 1;
      v = s1;
      j = k1;
      j = j - 2;
      v[j] = ~v[j];
      at_k_i = v;
    end
  endfunction

  logic [79:0] yc, yi;
  always @(posedge clk) begin
    edges = edges + 1;
    yc <= {incr(s, k), decr(s, k), flip(s, k), shr(s, k), none(s, k),
           one(s, k), even(s, k), rev(s, k), every3(s, k), at_k(s, k)};
    yi <= {incr_i(s, k), decr_i(s, k), flip_i(s, k), shr_i(s, k), none_i(s, k),
           one_i(s, k), even_i(s, k), rev_i(s, k), every3_i(s, k), at_k_i(s, k)};
  end

  task automatic check(input [7:0] sv, input [3:0] kv);
    @(negedge clk);
    s = sv;
    k = kv;
    @(negedge clk);
    if (yc !== yi) begin
      mism = mism + 1;
      $display("MISMATCH s=%b k=%b compiled=%b interpreted=%b", sv, kv, yc, yi);
    end
    $display("VAL s=%b k=%b %b", sv, kv, yc);
  endtask

  initial begin
    check(8'h00, 4'd0);
    check(8'h0f, 4'd5);
    check(8'hff, 4'd9);
    check(8'h5a, 4'd2);
    check(8'b1x0z_1100, 4'd3);
    check(8'bzzzz_0001, 4'bx);
    check(8'b0111_111x, 4'b1z00);
    check(8'h81, 4'd15);
    check(8'h7e, 4'd1);
    $display("MISMATCHES %0d CALLS %0d EDGES %0d", mism, calls, edges);
    $finish;
  end
endmodule
"#;

const NAMES: [&str; 10] = [
    "incr", "decr", "flip", "shr", "none", "one", "even", "rev", "every3", "at_k",
];

fn run(src: &str, tag: &str) -> (String, String) {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("loop_bit_helpers_{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sv = dir.join("t.sv");
    std::fs::write(&sv, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .env("XEZIM_PROBE_INLINE", "1")
        .env("XEZIM_COMPILE_METHODS", "0")
        .current_dir(&dir)
        .args(["--no-cache"])
        .arg(&sv)
        .output()
        .expect("run xezim");
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    );
    assert!(out.status.success(), "{stdout}{stderr}");
    (stdout, stderr)
}

#[test]
fn loop_bit_helpers_compile_and_match_the_interpreter() {
    let (stdout, stderr) = run(SRC, "match");
    assert!(stdout.contains("MISMATCHES 0 "), "{stdout}");
    // Hand-derived: incr, decr, flip, shr, none, one, even, rev, every3, at_k.
    for (s, k, vals) in [
        ("00000000", "0000", ["00000001", "11111111", "11111111", "x0000000", "00000000", "00000000", "00000000", "00000000", "10010010", "00000000"]),
        ("00001111", "0101", ["00010000", "00001110", "11110000", "x0000111", "00001111", "00001110", "00001010", "11110000", "10011111", "00000111"]),
        ("11111111", "1001", ["00000000", "11111110", "00000000", "x1111111", "11111111", "11111110", "10101010", "11111111", "11111111", "01111111"]),
        ("01011010", "0010", ["01011011", "01011001", "10100101", "x0101101", "01011010", "01011010", "00001010", "01011010", "11011010", "01011011"]),
        ("10000001", "1111", ["10000010", "10000000", "01111110", "x1000000", "10000001", "10000000", "10000000", "10000001", "10010011", "10000001"]),
        ("01111110", "0001", ["01111111", "01111101", "10000001", "x0111111", "01111110", "01111110", "00101010", "01111110", "11111110", "01111110"]),
    ] {
        let line = format!("VAL s={s} k={k} {}", vals.concat());
        assert!(stdout.contains(&line), "missing {line}:\n{stdout}");
    }
    // The impure twins ran once per edge each, on the interpreter.
    let field = |key: &str| -> u64 {
        let at = stdout.find(key).unwrap_or_else(|| panic!("no {key}: {stdout}")) + key.len();
        stdout[at..].split_whitespace().next().unwrap().parse().unwrap()
    };
    let (calls, edges) = (field("CALLS "), field("EDGES "));
    assert!(edges > 0 && calls == 10 * edges, "calls {calls}, edges {edges}");
    for n in NAMES {
        assert!(!stderr.contains(&format!("fn {n} reason=")), "{n} did not compile:\n{stderr}");
        assert!(stderr.contains(&format!("fn {n}_i reason=impure")), "{n}_i was inlined:\n{stderr}");
    }
}

const STATIC: &str = r#"
`timescale 1ns/1ns
module top;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [7:0] s = 8'd3;
  // Static locals that carry a value from one call to the next: a 2-state
  // accumulator read before it is written (also inside an `i = i + 1`
  // loop), and an initialized `static` counter.
  function [7:0] acc_f(input [7:0] s1);
    bit [7:0] acc;
    begin
      acc = acc + s1;
      acc_f = acc;
    end
  endfunction
  function [7:0] accl_f(input [7:0] s1);
    bit [7:0] acc;
    integer i;
    begin
      for (i = 0; i < 1; i = i + 1) acc = acc + s1;
      accl_f = acc;
    end
  endfunction
  function automatic [7:0] cnt_f(input [7:0] s1);
    static integer cnt = 0;
    begin
      cnt = cnt + 1;
      cnt_f = cnt;
    end
  endfunction
  logic [7:0] a, l, c;
  always @(posedge clk) begin
    a <= acc_f(s);
    l <= accl_f(s);
    c <= cnt_f(s);
  end
  initial begin
    #42 $display("STATIC acc=%0d accl=%0d cnt=%0d", a, l, c);
    $finish;
  end
endmodule
"#;

/// §6.21: a static local keeps its value from one call to the next, so a
/// function whose local can be read before the call writes it stays on the
/// interpreter, loop step or not. Four rising edges by 42: 3+3+3+3 and 4.
#[test]
fn static_locals_that_carry_stay_interpreted() {
    let (stdout, stderr) = run(STATIC, "static");
    assert!(stdout.contains("STATIC acc=12 accl=12 cnt=4"), "{stdout}");
    for n in ["acc_f", "accl_f", "cnt_f"] {
        assert!(stderr.contains(&format!("fn {n} reason=")), "{n} was inlined:\n{stderr}");
    }
}
