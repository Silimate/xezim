//! IEEE 1800-2023 §11.4.11 conditional operator with ambiguous (x/z) select,
//! and §10.10.3 braces nested in an unpacked array concatenation.
//!
//! §11.4.11: when the `?:` condition is x/z, BOTH operands are evaluated and
//! merged bit by bit (Table 11-21): a result bit keeps the operands' value
//! only where both agree on 0 or 1; every other pairing (incl. z/z) gives x.
//!
//! §10.10.3: each item of an unpacked array concatenation has a
//! self-determined type and the concatenation as a whole has none, so they
//! do not nest: a braced item is a vector or string concatenation, ONE
//! element (`q = {q, {pc, insn}}` appends one 64-bit element). A queue/array
//! operand of the concatenation splices its elements (`q2 = {0, q, 9}`).

use xezim::simulate;

fn flag(sim: &xezim::compiler::Simulator, name: &str) -> u64 {
    sim.get_signal(name)
        .or_else(|| sim.get_signal(&format!("tb.{}", name)))
        .unwrap_or_else(|| panic!("signal not found: {}", name))
        .to_u64()
        .unwrap_or_else(|| panic!("signal {} has x/z bits", name))
}

/// §11.4.11: x select merges the operands per bit — agreeing bits keep their
/// value, differing bits become x. Includes a same-bits case where the two
/// operands are identical, so NO bit may degrade to x.
#[test]
fn xselect_merges_operands_bitwise() {
    const SRC: &str = r#"
module tb;
  logic sel = 1'bx;
  logic selz = 1'bz;
  logic [3:0] r_diff, r_same, r_z, r_zz;
  bit pass_diff, pass_same, pass_z, pass_zz;
  initial begin
    // bit3: 1/1 -> 1, bit2: 1/0 -> x, bit1: 0/1 -> x, bit0: 0/0 -> 0
    r_diff = sel ? 4'b1100 : 4'b1010;
    pass_diff = (r_diff === 4'b1xx0);
    // Operands agree on every bit: merge must NOT introduce any x.
    r_same = sel ? 4'b1010 : 4'b1010;
    pass_same = (r_same === 4'b1010);
    // A z select is ambiguous too (has_unknown covers x and z).
    r_z = selz ? 4'b1110 : 4'b1010;
    pass_z = (r_z === 4'b1x10);
    // Table 11-21: z merged with z gives x, not z.
    r_zz = sel ? 4'b110z : 4'b101z;
    pass_zz = (r_zz === 4'b1xxx);
  end
endmodule
"#;
    let sim = simulate(SRC, 100).expect("simulate failed");
    assert_eq!(
        flag(&sim, "pass_diff"),
        1,
        "x-select must merge per bit: 1100/1010 -> 1xx0"
    );
    assert_eq!(
        flag(&sim, "pass_same"),
        1,
        "identical operands must merge with no x bits"
    );
    assert_eq!(
        flag(&sim, "pass_z"),
        1,
        "z select must merge like x: 1110/1010 -> 1x10"
    );
    assert_eq!(
        flag(&sim, "pass_zz"),
        1,
        "z/z bits must merge to x (Table 11-21)"
    );
}

/// §10.10.3: braces nested in an unpacked array concatenation are a vector
/// or string concatenation, one element of its self-determined type, at any
/// depth. The `q` case is the issue's reproducer (CHERIoT Ibex's `dii_if`),
/// where the reference simulator appends one element per statement, and
/// `SQ` is the §10.10.3 example. Array-of-arrays assignment patterns
/// `'{...}` keep one sub-array per inner pattern.
///
/// This test used to assert the opposite, `a = {1, {2, 3}}` assigning
/// `{1, 2, 3}`, citing §10.10.1. That clause's own example rules it out:
/// `A9 = {A3, {4,5,6,7,8,9} }; // illegal, {...} is not self-determined
/// here`. Braces around unsized literals are no legal vector concatenation
/// (§11.4.12) and an unpacked array concatenation cannot be an item
/// (§10.10.3), so that form has no meaning; with sized operands the braces
/// form one element.
#[test]
fn nested_braces_are_one_element() {
    const SRC: &str = r#"
module tb;
  logic [15:0] h[3];
  logic [15:0] d[3];
  logic [63:0] q[$];
  logic [31:0] pc, insn;
  string S1, S2;
  string SQ[$];
  int b[2][2];
  bit pass_h, pass_d, pass_q, pass_s, pass_b;
  initial begin
    h = {16'h1, {8'h23, 8'h45}, 16'h6};
    pass_h = (h[0] == 16'h0001) && (h[1] == 16'h2345) && (h[2] == 16'h0006);
    d = {16'h7, {8'h08, {4'h9, 4'ha}}, 16'hb};
    pass_d = (d[0] == 16'h0007) && (d[1] == 16'h089a) && (d[2] == 16'h000b);
    pc = 32'h8000_0080; insn = 32'h03d0_01db;
    q = {q, {pc, insn}};
    pc = 32'h8000_0084; insn = 32'h2002_05b7;
    q = {q, {pc, insn}};
    pass_q = (q.size() == 2) && (q[0] == 64'h80000080_03d001db)
             && (q[1] == 64'h80000084_200205b7);
    S1 = "S1";
    S2 = "S2";
    SQ = '{"element 0", "element 1"};
    SQ = {S1, SQ, {"element 3 is ", S2}};
    pass_s = (SQ.size() == 4) && (SQ[0] == "S1") && (SQ[1] == "element 0")
             && (SQ[2] == "element 1") && (SQ[3] == "element 3 is S2");
    // Array-of-arrays uses assignment patterns '{...}: each inner pattern is
    // one whole sub-array element.
    b = '{'{1, 2}, '{3, 4}};
    pass_b = (b[0][0] == 1) && (b[0][1] == 2) && (b[1][0] == 3) && (b[1][1] == 4);
  end
endmodule
"#;
    let sim = simulate(SRC, 100).expect("simulate failed");
    assert_eq!(
        flag(&sim, "pass_h"),
        1,
        "{{16'h1, {{8'h23, 8'h45}}, 16'h6}} is three elements"
    );
    assert_eq!(
        flag(&sim, "pass_d"),
        1,
        "deeper braces are still one vector concatenation"
    );
    assert_eq!(
        flag(&sim, "pass_q"),
        1,
        "q = {{q, {{pc, insn}}}} appends one 64-bit element"
    );
    assert_eq!(
        flag(&sim, "pass_s"),
        1,
        "the inner braces of the §10.10.3 example are one string"
    );
    assert_eq!(
        flag(&sim, "pass_b"),
        1,
        "'{{...}} sub-array patterns must not flatten"
    );
}

/// §10.10: a queue/array operand of an unpacked array concatenation splices
/// its elements — `q2 = {0, q, 9}`. Braces around a queue operand
/// (`{0, {q, 9}}`) are neither a vector concatenation (§11.4.12) nor a
/// nested unpacked array concatenation (§10.10.3); xezim keeps splicing
/// their operands, as before.
#[test]
fn queue_splice_in_concat_preserved() {
    const SRC: &str = r#"
module tb;
  int q[$];
  int q2[$];
  int q3[$];
  bit pass_q2, pass_q3;
  initial begin
    q = {5, 6};
    q2 = {0, q, 9};
    pass_q2 = (q2.size() == 4) && (q2[0] == 0) && (q2[1] == 5)
              && (q2[2] == 6) && (q2[3] == 9);
    // Braces around a queue operand still splice it.
    q3 = {0, {q, 9}};
    pass_q3 = (q3.size() == 4) && (q3[0] == 0) && (q3[1] == 5)
              && (q3[2] == 6) && (q3[3] == 9);
  end
endmodule
"#;
    let sim = simulate(SRC, 100).expect("simulate failed");
    assert_eq!(
        flag(&sim, "pass_q2"),
        1,
        "queue operand in concat must splice its elements"
    );
    assert_eq!(
        flag(&sim, "pass_q3"),
        1,
        "braces around a queue operand still splice"
    );
}

/// §10.10.3 through the other assignment path: a selected queue of a queue
/// array (`rows[0] = {rows[0], {pc, insn}}`) appends one element too, and
/// deciding whether braces hold a collection runs nothing, so an operand
/// with a side effect (`m[i++]`) is evaluated once.
#[test]
fn nested_braces_in_selected_queue_and_side_effects() {
    const SRC: &str = r#"
module tb;
  logic [63:0] rows[2][$];
  logic [31:0] pc, insn;
  byte m[int];
  int i;
  logic [15:0] q16[$];
  bit pass_rows, pass_once;
  initial begin
    pc = 32'h8000_0080; insn = 32'h03d0_01db;
    rows[0] = {rows[0], {pc, insn}};
    pc = 32'h8000_0084; insn = 32'h2002_05b7;
    rows[0] = {rows[0], {pc, insn}};
    pass_rows = (rows[0].size() == 2) && (rows[0][0] == 64'h80000080_03d001db)
                && (rows[0][1] == 64'h80000084_200205b7) && (rows[1].size() == 0);
    m[0] = 8'h11; m[1] = 8'h22;
    i = 0;
    q16 = {{m[i++], 8'h00}};
    pass_once = (i == 1) && (q16.size() == 1) && (q16[0] == 16'h1100);
  end
endmodule
"#;
    let sim = simulate(SRC, 100).expect("simulate failed");
    assert_eq!(
        flag(&sim, "pass_rows"),
        1,
        "rows[0] = {{rows[0], {{pc, insn}}}} appends one 64-bit element"
    );
    assert_eq!(
        flag(&sim, "pass_once"),
        1,
        "the braced operand m[i++] is evaluated once"
    );
}
