//! A package parameter of a packed multi-dimensional type selects elements.
//!
//! `PRESENT_SBOX4[x]` in OpenTitan's `prim_cipher_pkg` is a `[15:0][3:0]`
//! package parameter. It was registered as one flat vector, so the index
//! bit-selected and every lookup returned a single bit. Scrambled-RAM address
//! scrambling then produced X, and Ibex's icache tag RAM was written with X.
//! The same parameter declared in a module already worked, so each case checks
//! the scoped, imported, and constant-index forms against known elements.

use std::process::Command;

#[test]
fn package_packed_2d_parameter_index_selects_an_element() {
    let dir = std::env::temp_dir().join(format!("xezim_pkgp2d_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pkgp2d.sv");
    std::fs::write(
        &path,
        r#"package mp;
  parameter logic [3:0][3:0] Q = {4'h4, 4'h3, 4'h2, 4'h1};
  localparam logic [3:0][3:0] LQ = {4'h8, 4'h7, 4'h6, 4'h5};
endpackage
module tb;
  import mp::*;
  logic [1:0] n;
  logic [15:0] got;
  initial begin
    for (int i = 0; i < 4; i++) begin
      n = i[1:0];
      got = {mp::Q[n], mp::LQ[n], Q[n], LQ[n]};
      if (got !== {4'(i + 1), 4'(i + 5), 4'(i + 1), 4'(i + 5)}) $display("FAIL i=%0d got=%h", i, got);
      if (mp::Q[i] !== 4'(i + 1)) $display("FAIL const i=%0d got=%h", i, mp::Q[i]);
    end
    $display("DONE");
    $finish;
  end
endmodule
"#,
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--simulate", "-s", "tb", path.to_str().unwrap(), "--no-cache"])
        .output()
        .expect("run xezim");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("DONE"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
}

/// An `always_comb` that reads one element of a packed 2-D vector and writes
/// another must still wake when the element it reads is driven from outside.
#[test]
fn always_comb_reading_and_writing_elements_of_one_packed_vector_wakes_on_its_driver() {
    let dir = std::env::temp_dir().join(format!("xezim_pkcomb_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pkcomb.sv");
    std::fs::write(
        &path,
        r#"module tb;
  logic [7:0] a;
  logic [1:0][7:0] ds;
  logic [7:0] sb;
  assign ds[0] = a;
  always_comb begin sb = ds[0] ^ 8'h01; ds[1] = sb; end
  initial begin
    a = 8'h01; #1;
    if (ds[1] !== 8'h00) $display("FAIL first ds1=%h", ds[1]);
    a = 8'h22; #1;
    if (ds[1] !== 8'h23) $display("FAIL second ds1=%h", ds[1]);
    $display("DONE");
    $finish;
  end
endmodule
"#,
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--simulate", "-s", "tb", path.to_str().unwrap(), "--no-cache"])
        .output()
        .expect("run xezim");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("DONE"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
}
