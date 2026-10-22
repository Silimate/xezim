//! §27.6: a relative reference to an instance declared in a nested generate
//! names that iteration's instance. Ibex's icache RAM generate is
//! `if` / `for` / `if`, and `tag_bank.key_i` from the innermost block was
//! rewritten only through the outer scopes, so the read was x while the
//! absolute path saw the driven port.

use xezim::simulate;

fn messages(sim: &xezim::compiler::Simulator) -> Vec<String> {
    sim.output.iter().map(|o| o.message.clone()).collect()
}

#[test]
fn relative_port_read_inside_nested_generate_scopes() {
    let src = r#"
module prim_like (
  input logic [31:0] key_i
);
endmodule
module top;
  logic [31:0] key_q;
  initial key_q = 32'h00001111;
  if (1) begin : gen_rams
    for (genvar way = 0; way < 2; way++) begin : gen_rams_inner
      if (1) begin : gen_scramble_rams
        prim_like tag_bank (.key_i(key_q));
        initial begin
          #1;
          if (tag_bank.key_i !== 32'h00001111)
            $display("FAIL rel %0d %h", way, tag_bank.key_i);
          else
            $display("PASS rel %0d", way);
        end
      end
    end
  end
  initial begin
    #2;
    if (gen_rams.gen_rams_inner[0].gen_scramble_rams.tag_bank.key_i !== 32'h00001111)
      $display("FAIL abs %h", gen_rams.gen_rams_inner[0].gen_scramble_rams.tag_bank.key_i);
    else
      $display("PASS abs");
    $finish;
  end
endmodule
"#;
    let sim = simulate(src, 50).expect("sim");
    let msgs = messages(&sim);
    for want in ["PASS rel 0", "PASS rel 1", "PASS abs"] {
        assert!(
            msgs.iter().any(|m| m == want),
            "missing {want}; output: {msgs:?}"
        );
    }
    assert!(
        msgs.iter().all(|m| !m.starts_with("FAIL")),
        "output: {msgs:?}"
    );
}
