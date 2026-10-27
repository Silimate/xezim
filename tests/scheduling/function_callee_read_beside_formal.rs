//! IEEE 1800-2017 §9.2.2.2.1, §13.4: an `always_comb` is sensitive to what the
//! functions it calls read, nested calls included. A function's formals and
//! locals are left out of that set, but only its own: `g()` below reads the
//! module's `x`, and `f`'s formal `x` is another variable. Removing the frame
//! names after merging the callees' reads dropped the module `x`, so neither
//! the block nor the continuous assign re-ran when it changed.
//!
//! Fork patch over upstream's frame-name filter (9b1301eb); drop it when
//! upstream filters before merging callee reads.

use xezim::simulate;

fn messages(sim: &xezim::compiler::Simulator) -> Vec<String> {
    sim.output.iter().map(|o| o.message.trim().to_string()).collect()
}

#[test]
fn callee_read_of_a_name_the_caller_binds_as_a_formal() {
    let sim = simulate(
        r#"
module top;
  logic x, y, comb_out, assign_out;
  function automatic logic g();
    return x;
  endfunction
  function automatic logic f(input logic x);
    return g();
  endfunction
  always_comb comb_out = f(y);
  assign assign_out = f(y);
  initial begin
    x = 0; y = 0;
    #1 x = 1;
    #1 $display("B comb=%b assign=%b", comb_out, assign_out);
    x = 0;
    #1 $display("C comb=%b assign=%b", comb_out, assign_out);
  end
endmodule
"#,
        10,
    )
    .expect("sim");
    let msgs = messages(&sim);
    assert!(msgs.iter().any(|m| m == "B comb=1 assign=1"), "{msgs:?}");
    assert!(msgs.iter().any(|m| m == "C comb=0 assign=0"), "{msgs:?}");
}
