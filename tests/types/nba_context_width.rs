//! §11.6.1, §10.4.2, §9.4.5 — the right-hand side of a nonblocking
//! assignment, and of an assignment with an intra-assignment event control,
//! takes the target's width as its context, as a blocking assignment's does.
//!
//! The interpreter evaluated those right-hand sides self-determined, so
//! `s <= a + b` with 16-bit `a`, `b` and an 18-bit `s` dropped the carry, and
//! `s <= -a` negated `a` before extending it. Compiled bytecode was right; the
//! interpreter runs a statement the compiler can't take, such as a `for` or
//! `repeat` loop bounded by `$clog2(...)` whose body also writes a part-select
//! (the pipelined adder of a CVDP matrix multiplier). The reference simulator
//! prints 1fffe for every sum here and 30001 for `-a`.

use xezim::simulate;

fn hex(sim: &xezim::compiler::Simulator, n: &str) -> u64 {
    sim.get_signal(&format!("tb.{n}"))
        .or_else(|| sim.get_signal(n))
        .unwrap_or_else(|| panic!("signal not found: {n}"))
        .to_u64()
        .unwrap_or_else(|| panic!("{n} is x/z"))
}

const SRC: &str = r#"
module tb;
  logic clk = 0;
  logic [15:0] a = 16'hFFFF, b = 16'hFFFF;
  logic [17:0] s_for, s_rep, s_ev, s_bev, s_cat, s_neg;
  logic [7:0]  s_narrow;
  logic [35:0] v;
  logic [127:0] v2, v3;
  always_ff @(posedge clk)
    for (int l = 0; l < $clog2(4); l++) begin
      s_for <= a + b;
      v[l*18 +: 18] <= a + b;
    end
  always_ff @(posedge clk)
    repeat ($clog2(4)) begin
      s_rep <= a + b;
      v2[40 +: 18] <= 0;
    end
  // Guards: a narrower target keeps the low bits; a concatenation is
  // self-determined, so its sum stays 16 bits.
  always_ff @(posedge clk)
    for (int l = 0; l < $clog2(4); l++) begin
      s_narrow <= a + b;
      s_cat <= {a + b};
      s_neg <= -a;
      v3[80 +: 18] <= 0;
    end
  initial begin
    s_ev <= @(posedge clk) a + b;
    s_bev = @(posedge clk) a + b;
  end
  initial begin
    #1 clk = 1;
    #1 clk = 0;
  end
endmodule
"#;

#[test]
fn nonblocking_right_hand_sides_take_the_target_width() {
    let sim = simulate(SRC, 10).expect("simulate failed");
    for n in ["s_for", "s_rep", "s_ev", "s_bev"] {
        assert_eq!(
            hex(&sim, n),
            0x1FFFE,
            "{n}: a + b in an 18-bit context keeps the carry"
        );
    }
    assert_eq!(
        hex(&sim, "v"),
        (0x1FFFE << 18) | 0x1FFFE,
        "each v[l*18 +: 18] keeps the carry"
    );
    assert_eq!(hex(&sim, "s_neg"), 0x30001, "-a extends a to 18 bits first");
    assert_eq!(
        hex(&sim, "s_narrow"),
        0xFE,
        "an 8-bit target keeps the low bits"
    );
    assert_eq!(
        hex(&sim, "s_cat"),
        0x0FFFE,
        "a concatenation is self-determined"
    );
}
