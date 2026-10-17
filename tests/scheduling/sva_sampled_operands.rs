//! §16.9.3 — sampled-value functions of an operand that is not a plain
//! signal name.
//!
//! The operand of `$past`, `$rose`, `$fell`, `$stable` and `$changed` is any
//! expression; its sampled value is the expression over the sampled values
//! of its variables (§16.5.1). Concurrent assertions kept a history only for
//! a bare signal name: with a bit- or part-select, a concatenation, a struct
//! member, a function call or any other expression, `$past` returned 0,
//! `$rose` never held and `$stable` always did, because the history lookup
//! read a signal with an empty name. A call nested in a select or a
//! concatenation (`w[$past(i)]`) had no history at all and read the current
//! value, and the gate of `$past(e, n, gate)` was ignored.
//!
//! Each check compares against shadow flops, which hold the value an operand
//! had at earlier ticks. The commercial simulator prints `PASS` (and the same
//! `COUNTS` and `RESULT` lines) for every design here. A sampled-value call
//! in `disable iff` needs an explicit clock (§16.9.3), so `a_dis` gives one.

use std::path::PathBuf;
use std::process::Command;

fn run(name: &str, top: &str, src: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sva_sampled_operands");
    std::fs::create_dir_all(&dir).unwrap();
    let sv = dir.join(format!("{name}.sv"));
    std::fs::write(&sv, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--simulate", "-s", top, "--no-cache", sv.to_str().unwrap()])
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "run failed:\n{text}");
    text
}

/// The design's report: its PASS, FAIL, COUNTS and RESULT lines.
fn report(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            ["PASS", "FAIL", "COUNTS", "RESULT"]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .collect()
}

/// Selects with constant and variable indices, `+:`/`-:` part-selects,
/// concatenations, struct members, function calls and other expressions as
/// operands; `$past` with a depth and with a gate; nested calls; and the same
/// functions in `disable iff`, named sequence and property arguments, `cover
/// property` and a procedural concurrent assertion.
#[test]
fn sampled_value_functions_of_selects_and_expressions() {
    let text = run(
        "operands",
        "t",
        r#"
module t;
  logic clk = 0;
  always #5 clk = ~clk;
  typedef struct packed { logic [1:0] hi; logic [1:0] lo; } pair_t;
  function automatic logic [1:0] f(logic [3:0] x); return x[3:2] ^ x[1:0]; endfunction
  logic [7:0] lfsr = 8'h5a;
  logic [7:0] w = 0, w1 = 0;
  logic [3:0] v = 0, v1 = 0, v2 = 0, vg = 0;
  logic [1:0] vg2 = 0, vga = 0, vgb = 0;
  logic a = 0, c = 0, en = 0, a1 = 0, c1 = 0, en_seen = 0;
  logic [1:0] i = 0, i1 = 0;
  pair_t s = '0, s1 = '0;
  int ticks = 0, fails = 0, en_n = 0;
  int n_rose = 0, m_rose = 0, n_seq = 0, n_dis = 0, m_dis = 0, n_fell = 0, m_fell = 0;
  int n_cov = 0, m_cov = 0, n_nest = 0, m_nest = 0;
  always @(posedge clk) begin
    ticks <= ticks + 1;
    v1 <= v; v2 <= v1; w1 <= w; a1 <= a; c1 <= c; i1 <= i; s1 <= s;
    if (en) begin vg <= v; vg2 <= v[2:1]; en_seen <= 1; vga <= v[1:0]; vgb <= vga; en_n <= en_n + 1; end
    if (ticks >= 1 && v[1] && !v1[1]) m_rose++;
    if (ticks >= 1 && !v[i] && v1[i1]) m_fell++;
    if (!(ticks >= 1 && v[3] && !v1[3])) m_dis++;
    if (ticks >= 1 && (s.lo ^ v[1:0]) != (s1.lo ^ v1[1:0])) m_cov++;
    if (ticks >= 2 && v1[0] && !v2[0]) m_nest++;
  end
  initial begin
    repeat (60) begin
      @(negedge clk);
      lfsr = {lfsr[6:0], lfsr[7] ^ lfsr[5] ^ lfsr[4] ^ lfsr[3]};
      v = lfsr[3:0]; w = {lfsr[2:0], lfsr[7:3]}; a = lfsr[4]; c = lfsr[6]; en = lfsr[5] & lfsr[1];
      i = lfsr[7:6]; s = {lfsr[1:0], lfsr[5:4]};
    end
    @(negedge clk);
    if (n_rose != m_rose) begin fails++; $display("FAIL n_rose=%0d m_rose=%0d", n_rose, m_rose); end
    if (n_seq != m_rose) begin fails++; $display("FAIL n_seq=%0d m_rose=%0d", n_seq, m_rose); end
    if (n_fell != m_fell) begin fails++; $display("FAIL n_fell=%0d m_fell=%0d", n_fell, m_fell); end
    if (n_dis != m_dis) begin fails++; $display("FAIL n_dis=%0d m_dis=%0d", n_dis, m_dis); end
    if (n_cov != m_cov) begin fails++; $display("FAIL n_cov=%0d m_cov=%0d", n_cov, m_cov); end
    if (n_nest != m_nest) begin fails++; $display("FAIL n_nest=%0d m_nest=%0d", n_nest, m_nest); end
    $display("COUNTS rose=%0d fell=%0d dis=%0d cov=%0d nest=%0d", m_rose, m_fell, m_dis, m_cov, m_nest);
    if (fails == 0) $display("PASS"); else $display("FAIL %0d", fails);
    $finish;
  end
  `define CHK(name, prop) name: assert property (@(posedge clk) prop) else begin fails++; $display("FAIL %s at %0t", `"name`", $time); end
  // §16.9.3: any expression is an operand; its sampled value is the
  // expression over sampled values (§16.5.1).
  `CHK(p_bit,   ticks < 1 || $past(v[2]) == v1[2])
  `CHK(p_part,  ticks < 1 || $past(v[2:1]) == v1[2:1])
  `CHK(p_var,   ticks < 1 || $past(v[i]) == v1[i1])
  `CHK(p_up,    ticks < 1 || $past(w[i +: 4]) == w1[i1 +: 4])
  `CHK(p_down,  ticks < 1 || $past(w[i + 4 -: 3]) == w1[i1 + 4 -: 3])
  `CHK(p_cat,   ticks < 1 || $past({a, v[0], c}) == {a1, v1[0], c1})
  `CHK(p_mem,   ticks < 1 || $past(s.hi) == s1.hi)
  `CHK(p_func,  ticks < 1 || $past(f(v)) == f(v1))
  `CHK(p_and,   ticks < 1 || $past(a & c) == (a1 & c1))
  `CHK(p_add,   ticks < 1 || $past(v + 4'd1) == v1 + 4'd1)
  `CHK(p_wide,  ticks < 1 || $past(v + 5'd1) == v1 + 5'd1)
  `CHK(p_two,   ticks < 2 || $past(v[3:2], 2) == v2[3:2])
  `CHK(p_gate,  !en_seen || $past(v[2:1], 1, en) == vg2)
  `CHK(p_gid,   !en_seen || $past(v, 1, en) == vg)
  `CHK(p_gate2, en_n < 2 || $past(v[1:0], 2, en) == vgb)
  `CHK(r_bit,   ticks < 1 || $rose(v[1]) == (v[1] && !v1[1]))
  `CHK(r_expr,  ticks < 1 || $rose(a & c) == (a & c & !(a1 & c1)))
  `CHK(f_var,   ticks < 1 || $fell(v[i]) == (!v[i] && v1[i1]))
  `CHK(s_part,  ticks < 1 || $stable(v[2:1]) == (v[2:1] == v1[2:1]))
  `CHK(c_cat,   ticks < 1 || $changed({a, c}) == ({a, c} != {a1, c1}))
  `CHK(c_mem,   ticks < 1 || $changed(s.lo) == (s.lo != s1.lo))
  `CHK(n_past,  ticks < 2 || $past($rose(v[0])) == (v1[0] && !v2[0]))
  `CHK(n_imp,   v[0] |=> $past(v[0]))
  `CHK(p_inidx, ticks < 1 || w[$past(i)] == w[i1])
  `CHK(p_incat, ticks < 1 || {$past(a), c} == {a1, c})
  c_rose: cover property (@(posedge clk) ticks >= 1 && $rose(v[1])) n_rose++;
  c_fell: cover property (@(posedge clk) ticks >= 1 && $fell(v[i])) n_fell++;
  c_expr: cover property (@(posedge clk) ticks >= 1 && $changed(s.lo ^ v[1:0])) n_cov++;
  c_nest: cover property (@(posedge clk) ticks >= 2 && $past($rose(v[0]))) n_nest++;
  // disable iff: an attempt is cancelled exactly at the ticks v[3] rises.
  a_dis: assert property (@(posedge clk) disable iff (ticks >= 1 && $rose(v[3], @(posedge clk))) 1) n_dis++;
  // Sequence and property arguments.
  sequence s_rise(x); $rose(x); endsequence
  property p_was(x, y); ticks >= 1 |-> $past(x) == y; endproperty
  c_seq: cover property (@(posedge clk) ticks >= 1 ##0 s_rise(v[1])) n_seq++;
  `CHK(a_seq,  ticks >= 1 ##0 s_rise(v[2]) |-> v[2] && !v1[2])
  `CHK(a_prop, p_was(v[2:1], v1[2:1]))
  `CHK(a_two,  p_was(v[1:0], v1[1:0]) and p_was(v[3:2], v1[3:2]))
  // A procedural concurrent assertion takes its clock from the block.
  always @(posedge clk) begin
    a_proc: assert property (ticks < 1 || $past(v[3:1]) == v1[3:1])
      else begin fails++; $display("FAIL a_proc at %0t", $time); end
  end
endmodule
"#,
    );
    assert_eq!(
        report(&text),
        ["COUNTS rose=17 fell=11 dis=44 cov=40 nest=17", "PASS"],
        "report:\n{text}"
    );
}

/// The testcase of the report: the counts of covers on a bit, a part-select
/// and an expression.
#[test]
fn covers_count_matches_of_select_and_expression_operands() {
    let text = run(
        "covers",
        "top",
        r#"
module top;
  logic clk = 0;
  logic [3:0] d = 0;
  int r_bit = 0, r_id = 0, p_sel = 0, p_expr = 0, n = 0;
  always #5 clk = ~clk;
  initial begin
    repeat (8) begin @(negedge clk); d = d + 1; end
    @(negedge clk);
    $display("RESULT n=%0d rose_bit=%0d rose_id=%0d past_sel=%0d past_expr=%0d", n, r_bit, r_id, p_sel, p_expr);
    $finish;
  end
  c_n: cover property (@(posedge clk) 1) n++;
  c_rb: cover property (@(posedge clk) $rose(d[0])) r_bit++;
  c_ri: cover property (@(posedge clk) $rose(d)) r_id++;
  c_ps: cover property (@(posedge clk) $past(d[1:0]) == 2'b11) p_sel++;
  c_pe: cover property (@(posedge clk) $past(d & 4'b0001) == 1) p_expr++;
endmodule
"#,
    );
    assert_eq!(
        report(&text),
        ["RESULT n=9 rose_bit=4 rose_id=4 past_sel=2 past_expr=4"],
        "report:\n{text}"
    );
}

/// A struct member is sampled with its variable, in an operand and in the
/// property body alike: here the struct is a flop, written by an NBA at each
/// tick, so a read after the tick's updates saw the next value.
#[test]
fn struct_members_of_a_flop_are_sampled() {
    let text = run(
        "members",
        "t",
        r#"
module t;
  logic clk = 0;
  always #5 clk = ~clk;
  typedef struct packed { logic [1:0] hi; logic [1:0] lo; } pair_t;
  pair_t s = '0;                   // a flop: updated by an NBA at each tick
  logic [3:0] d = 0, q = 0;
  int ticks = 0, fails = 0;
  always @(posedge clk) begin ticks <= ticks + 1; s <= d; q <= s; end
  initial begin
    repeat (8) begin @(negedge clk); d = d * 5 + 3; end
    @(negedge clk);
    if (fails == 0) $display("PASS"); else $display("FAIL %0d", fails);
    $finish;
  end
  // q holds s one tick back, and s holds d one tick back.
  a_op:   assert property (@(posedge clk) ticks < 2 || $past(s.hi) == q[3:2])
            else begin fails++; $display("FAIL a_op %0t", $time); end
  a_body: assert property (@(posedge clk) ticks < 1 || s.lo == $past(d[1:0]))
            else begin fails++; $display("FAIL a_body %0t", $time); end
  a_chg:  assert property (@(posedge clk) ticks < 2 || $changed(s.hi) == (s.hi != q[3:2]))
            else begin fails++; $display("FAIL a_chg %0t", $time); end
endmodule
"#,
    );
    assert_eq!(report(&text), ["PASS"], "report:\n{text}");
}

/// Each instance keeps its own history, and the operand is sized by the
/// instance's parameters.
#[test]
fn each_instance_keeps_its_own_operand_history() {
    let text = run(
        "instances",
        "t",
        r#"
module m #(parameter int W = 4, parameter int SEED = 1) (input logic clk, output int fails);
  logic [W-1:0] v = 0, v1 = 0;
  int ticks = 0;
  initial fails = 0;
  always @(posedge clk) begin ticks <= ticks + 1; v <= v * 5 + SEED; v1 <= v; end
  a_inst: assert property (@(posedge clk) ticks < 2 || $past(v[W-1:1]) == v1[W-1:1])
    else begin fails++; $display("FAIL a_inst %m %0t", $time); end
  a_rose: assert property (@(posedge clk) ticks < 2 || $rose(v[0]) == (v[0] && !v1[0]))
    else begin fails++; $display("FAIL a_rose %m %0t", $time); end
endmodule
module t;
  logic clk = 0;
  always #5 clk = ~clk;
  int f1, f2;
  m #(.W(4), .SEED(3)) u1 (.clk(clk), .fails(f1));
  m #(.W(6), .SEED(7)) u2 (.clk(clk), .fails(f2));
  initial begin
    #300;
    if (f1 == 0 && f2 == 0) $display("PASS"); else $display("FAIL %0d %0d", f1, f2);
    $finish;
  end
endmodule
"#,
    );
    assert_eq!(report(&text), ["PASS"], "report:\n{text}");
}
