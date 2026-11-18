//! §7.2, §7.4.2: a member write through constant element selects of a packed
//! struct (`st.e[1].dw[0].value <= d`) is one static bit range of the
//! container, which elaboration lists in its flattened layout. The compiler
//! sent every such NBA and blocking write to the AST interpreter
//! (`nba_member_access`, `blocking_target_member_access`); a key-vault-shaped
//! design with 384 of them in generate loops ran about 100x slower than its
//! flat-array twin. Expected values are Verilator's (5.046).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn constant_element_member_writes_compile_and_match() {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/perf/packed_member_path_store.sv");
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args([
            "--simulate",
            "-s",
            "packed_member_path_store_check",
            source.to_str().unwrap(),
            "--no-cache",
        ])
        .env("XEZIM_FALLBACK_SITES", "1")
        .output()
        .expect("run packed member path workload");

    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(
        output.status.success(),
        "packed member path workload failed:\n{text}"
    );
    assert!(
        text.contains("st=23222120 c=2120 a=9a0a1a2 top_st=77556688 rd=22 s2_o=99000000 s2=11")
            && text.contains("PACKED_MEMBER_PATH_OK"),
        "packed member path values did not match:\n{text}"
    );
    assert!(
        !text.contains("member_access"),
        "a member write through element selects fell back to the interpreter:\n{text}"
    );
}

/// §11.6.1: a sized index expression keeps its width (`1'b1 + 1'b1` is 0,
/// so the NBA writes `e[0]`), and a function's own struct shadows the
/// module signal of the same name. Both stay on the interpreter's path.
#[test]
fn index_width_and_a_shadowing_local() {
    let got: Vec<String> = xezim::simulate(
        r#"
// width of an index expression, and a function-local container that shadows a module signal
package gp;
  typedef struct packed { logic [7:0] value; } dw_t;
  typedef struct packed { dw_t [1:0] dw; } nest_t;
  typedef struct packed { nest_t [2:0] e; } nest3_t;
endpackage
module gsub import gp::*; (input logic clk, output nest3_t st, output logic [7:0] fr);
  nest3_t s;
  function automatic logic [7:0] rd_local();
    nest3_t s;
    s = '0;
    s.e[1].dw[0].value = 8'h77;
    return s.e[1].dw[0].value;
  endfunction
  always_ff @(posedge clk) begin
    st <= '0;
    st.e[1'b1 + 1'b1].dw[0].value <= 8'h5A;
  end
  initial begin s = '0; s.e[1].dw[0].value = 8'h11; end
  always_comb fr = rd_local();
endmodule
module gtop;
  logic clk = 0; gp::nest3_t st; logic [7:0] fr;
  gsub u (.clk(clk), .st(st), .fr(fr));
  initial begin #1 clk = 1; #1 $display("g1: st=%h fr=%h", st, fr); end
endmodule
"#,
        100,
    )
    .expect("simulate failed")
    .output
    .iter()
    .map(|o| o.message.clone())
    .collect();
    let want = "g1: st=00000000005a fr=77";
    assert!(
        got.iter().any(|l| l == want),
        "missing `{want}` in {got:#?}"
    );
}
