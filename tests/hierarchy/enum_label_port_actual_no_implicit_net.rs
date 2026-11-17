//! Issue #305: §6.19 + §6.10: an enum label used in a port actual of an
//! instance inside a module (`.din(miss_state == STREAM)`, the enum typedef
//! declared in that module) names a declared constant, not a net.
//!
//! The pass that creates §6.10 implicit nets for undeclared names in an
//! instance's port actuals checked each name against the instantiating
//! module's declarations, but an enum's labels are registered only when that
//! module's items are, after the pass. Every such label got a spurious
//! "implicit 1-bit net created" warning, under `default_nettype none` too,
//! where §22.8 allows no implicit net at all. The value bound was correct.
//!
//! The same candidate list also made a label from a package (`import pk::*`,
//! §26.3) or `$unit` a local name of the module being inlined, so every use
//! of it in that module took the instance path (`m.l.SYS_CSR`), which names
//! nothing: two or more levels down, `op == SYS_CSR` read x there, silently.

use std::process::Command;

/// `{CONN}` is the `ff` instance's connection list, `{PRE}` comes first in
/// the file.
const SRC: &str = r#"{PRE}
module ff (input logic clk, input logic din, output logic dout);
  always_ff @(posedge clk) dout <= din;
endmodule
module thr (input logic clk);
  typedef enum logic [3:0] {IDLE = 4'd0, STREAM = 4'd6} st_t;
  st_t miss_state = STREAM;
  enum logic [1:0] {DA, DB} d = DB;
  logic dout;
  ff u ({CONN});
  if (1) begin : g
    logic dout;
    ff u2 (.clk(clk), .din(miss_state != IDLE && d == DB), .dout(dout));
  end
endmodule
module t;
  logic clk = 0;
  always #5 clk = ~clk;
  thr inst (.clk(clk));
  initial begin
    #22;
    if (inst.dout === 1'b1 && inst.g.dout === 1'b1) $display("PASS");
    else $display("FAIL %b %b", inst.dout, inst.g.dout);
    $finish;
  end
endmodule
"#;

fn run(tag: &str, pre: &str, conn: &str) {
    let dir = std::env::temp_dir().join(format!(
        "xezim_enum_port_actual_{tag}_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let sv = dir.join("enum_actual.sv");
    std::fs::write(&sv, SRC.replace("{PRE}", pre).replace("{CONN}", conn)).expect("write sv");
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .env("XEZIM_NO_CACHE", "1")
        .args(["-s", "t"])
        .arg(&sv)
        .output()
        .expect("run xezim");
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("PASS"),
        "{tag}: stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("implicit") && !stdout.contains("implicit"),
        "{tag}: spurious implicit-net diagnostic:\n{stderr}{stdout}"
    );
}

#[test]
fn enum_label_in_named_connection_is_no_implicit_net() {
    run("named", "", ".clk(clk), .din(miss_state == STREAM), .dout(dout)");
}

#[test]
fn enum_label_beside_implicit_name_connections() {
    run("dotname", "", ".din(miss_state == STREAM), .dout, .clk");
    run("dotstar", "", ".*, .din((miss_state == STREAM))");
}

#[test]
fn enum_label_in_ordered_connection_is_no_implicit_net() {
    run("ordered", "", "clk, miss_state == STREAM, dout");
}

/// §22.8: under `default_nettype none` no implicit net may be created, and
/// a declared label must not be reported as one.
#[test]
fn enum_label_under_default_nettype_none() {
    run(
        "none",
        "`default_nettype none",
        ".clk(clk), .din(miss_state == STREAM), .dout(dout)",
    );
}

/// One leaf per way a label can be visible in the module: a wildcard import
/// in the header and in the body, an explicit import, a qualified name, a
/// module-local typedef and a `$unit` typedef. Each uses its label in a port
/// actual and directly, and must read it there.
const IMPORTED: &str = r#"
package pk;
  typedef enum logic [2:0] { SYS_CSR, SYS_ERET } sys_op_e;
endpackage
typedef enum logic [1:0] { U_A, U_B } u_e;
module probe (input i);
endmodule
`define CHECK(LBL, OTHER) \
  logic [2:0] op = 3'd0; \
  probe p (.i(op == LBL)); \
  initial #1 $display("%s %m direct=%b port=%b", \
    ((op == LBL) === 1'b1 && (op != OTHER) === 1'b1 && p.i === 1'b1) ? "PASS" : "FAIL", \
    op == LBL, p.i);
module leaf_wild_hdr import pk::*; ();
  `CHECK(SYS_CSR, SYS_ERET)
endmodule
module leaf_wild_body;
  import pk::*;
  `CHECK(SYS_CSR, SYS_ERET)
endmodule
module leaf_explicit;
  import pk::SYS_CSR;
  import pk::SYS_ERET;
  `CHECK(SYS_CSR, SYS_ERET)
endmodule
module leaf_qualified;
  `CHECK(pk::SYS_CSR, pk::SYS_ERET)
endmodule
module leaf_local;
  typedef enum logic [2:0] { L_A, L_B } l_e;
  `CHECK(L_A, L_B)
endmodule
module leaf_unit;
  `CHECK(U_A, U_B)
endmodule
module all_leaves;
  leaf_wild_hdr wh(); leaf_wild_body wb(); leaf_explicit ex();
  leaf_qualified q(); leaf_local lo(); leaf_unit un();
endmodule
module depth2; all_leaves a(); endmodule
module depth3; depth2 b(); endmodule
"#;

/// The leaves one, two and three levels below the top.
#[test]
fn imported_enum_label_in_port_actual_reads_at_every_depth() {
    let dir = std::env::temp_dir().join(format!(
        "xezim_enum_port_actual_imported_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let sv = dir.join("imported.sv");
    std::fs::write(&sv, IMPORTED).expect("write sv");
    for top in ["all_leaves", "depth2", "depth3"] {
        let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
            .env("XEZIM_NO_CACHE", "1")
            .args(["-s", top])
            .arg(&sv)
            .output()
            .expect("run xezim");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            stdout.matches("PASS").count(),
            6,
            "-s {top}: stdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("implicit") && !stdout.contains("implicit"),
            "-s {top}: spurious implicit-net diagnostic:\n{stderr}{stdout}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
