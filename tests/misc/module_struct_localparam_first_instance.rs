//! A module-local struct localparam is packed for the FIRST instance too.
//!
//! The localparam's struct typedef is declared in the module body. It was only
//! visible to the parameter packer after the first instance had been settled,
//! so the first instance read 0 and every later one read the right value. In
//! Ibex this left the main core's `mstatus` reset value 0 while the lockstep
//! shadow core's was 0x80.

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
fn struct_localparam_is_packed_in_every_instance() {
    let text = run(
        "stlp",
        r#"module leaf #(parameter logic [5:0] RV = '0) (output logic [5:0] q);
  assign q = RV;
endmodule
module core (output logic [5:0] q, output logic [5:0] r);
  typedef struct packed { logic mie; logic mpie; logic [1:0] mpp; logic mprv; logic tw; } st_t;
  localparam st_t RST = '{mie: 1'b0, mpie: 1'b1, mpp: 2'b00, mprv: 1'b0, tw: 1'b0};
  leaf #(.RV(RST)) u (.q(q));
  assign r = RST;
endmodule
module tb;
  logic [5:0] qa, ra, qb, rb;
  core a (.q(qa), .r(ra));
  core b (.q(qb), .r(rb));
  initial begin
    #1;
    if (qa !== 6'b010000 || ra !== 6'b010000) $display("FAIL first q=%b r=%b", qa, ra);
    if (qb !== 6'b010000 || rb !== 6'b010000) $display("FAIL second q=%b r=%b", qb, rb);
    $display("DONE");
    $finish;
  end
endmodule
"#,
    );
    assert!(text.contains("DONE"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
}
