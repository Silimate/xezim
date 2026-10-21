//! A packed 2-D signal whose declared bounds name instance parameters.
//!
//! The declaration's dimensions are evaluated against the flat parameter
//! table by bare name. For an instance stored as `inst.PARAM`, a bound that
//! names a parameter with no bare entry fails, while one that names a
//! parameter shadowed by an unrelated top-level parameter of the same name
//! resolves to the wrong value, so the declared geometry came back as one
//! 16-bit slice. `r[c][0]` then wrote bit `c` instead of bit `c * 16`, which
//! left bits 16 and 32 at X in an Ibex PMP. Values are made distinct per bit
//! so a write that lands on the wrong bit cannot hide behind a uniform value.

use std::process::Command;

#[test]
fn packed_2d_index_with_instance_param_bounds_writes_the_right_bit() {
    let dir = std::env::temp_dir().join(format!("xezim_pk2d_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pk2d.sv");
    std::fs::write(&path, r#"package pk;
  typedef struct packed { logic a; logic [1:0] m; } cfg_t;
endpackage
module sub #(parameter int unsigned PMPNumChan = 2, parameter int unsigned PMPNumRegions = 4) (
  input pk::cfg_t cfg_i [PMPNumRegions]
);
  logic [PMPNumChan-1:0][PMPNumRegions-1:0] r;
  logic [PMPNumChan-1:0] en;
  function automatic logic f(input pk::cfg_t cfg, input int c, input logic e);
    logic res;
    res = 1'b0;
    case (cfg.m)
      2'b00: res = e;
      2'b01: res = (c % 2 == 0);
      default: res = cfg.a ^ e;
    endcase
    return res;
  endfunction
  for (genvar c = 0; c < PMPNumChan; c++) begin : g_c
    for (genvar k = 0; k < PMPNumRegions; k++) begin : g_k
      assign r[c][k] = f(cfg_i[k], c, en[c]);
    end
  end
endmodule

module core #(parameter bit On = 1) ();
  pk::cfg_t cfg [16];
  if (On) begin : g
    sub #(.PMPNumChan(3), .PMPNumRegions(16)) u (.cfg_i(cfg));
  end
  initial begin
    #5;
    for (int k = 0; k < 16; k++) cfg[k] = {k[0], k[2:1]} ;
  end
endmodule

module tb;
  parameter int PMPNumRegions = 16;
  core c0 ();
  logic [47:0] expect_v;
  initial begin
    c0.g.u.en = 3'b101;
    #10;
    for (int c = 0; c < 3; c++) for (int k = 0; k < 16; k++) begin
      pk::cfg_t q; bit e; q = {k[0], k[2:1]}; e = c0.g.u.en[c];
      case (q.m) 2'b00: expect_v[c*16+k] = e; 2'b01: expect_v[c*16+k] = (c%2==0); default: expect_v[c*16+k] = q.a ^ e; endcase
    end
    if (c0.g.u.r === expect_v) $display("PASS r=%h", c0.g.u.r);
    else $display("FAIL r=%b expect=%b", c0.g.u.r, expect_v);
    $finish;
  end
endmodule
"#).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args([
            "--simulate",
            "-s",
            "tb",
            path.to_str().unwrap(),
            "--no-cache",
            "--max-time",
            "1000",
        ])
        .output()
        .expect("run xezim");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("PASS r="), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
}
