//! IEEE 1800-2017 §9.2.2.2.1: an `always_comb` that reads a whole struct and
//! writes one of its members stays sensitive to the struct: only the written
//! member is excluded, and `out = s` reads every member. Dropping the base
//! along with the self-written member left `out` stale when another process
//! changed `s.b`.
//!
//! Fork patch over upstream's self-written member rule (1c913764); drop it
//! when upstream keeps whole-value reads.

use xezim::simulate;

fn messages(sim: &xezim::compiler::Simulator) -> Vec<String> {
    sim.output.iter().map(|o| o.message.trim().to_string()).collect()
}

#[test]
fn whole_struct_read_survives_a_self_written_member() {
    let sim = simulate(
        r#"
module top;
  typedef struct packed { logic [3:0] a; logic [3:0] b; } s_t;
  s_t s;
  s_t out;
  always_comb begin
    out = s;
    s.a = s.a;
  end
  initial begin
    s.b = 4'h0;
    #1 s.b = 4'h5;
    #1 $display("B out.b=%h", out.b);
    s.b = 4'h9;
    #1 $display("C out.b=%h", out.b);
  end
endmodule
"#,
        10,
    )
    .expect("sim");
    let msgs = messages(&sim);
    assert!(msgs.iter().any(|m| m == "B out.b=5"), "{msgs:?}");
    assert!(msgs.iter().any(|m| m == "C out.b=9"), "{msgs:?}");
}
