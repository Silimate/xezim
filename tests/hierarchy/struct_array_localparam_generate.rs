//! §6.20.2, §7.2.1, §27.5: an unpacked-array-of-struct `localparam` in a
//! module below the top, read by the module's generate conditions
//! (`if (Info[k].variant == ...)`) and by its continuous assignments
//! (issue #342, the form with the parameter declared in the module).
//!
//! The instance's body parameters are folded before its generate constructs
//! are expanded. An unpacked array's elements were folded with the element
//! type as written, so a struct typedef of the same body, not bound yet,
//! made every element 0; and no element layout was registered under the
//! array's name, so `Info[k].variant` did not resolve in a generate
//! condition even with a package type. Every branch fell to `else`, and the
//! assignments in the others were lost. The fold now inlines the instance's
//! own types, as the scalar `localparam` arm does, and registers the
//! element layout.
//!
//! Expected values are Verilator's (5.046).

use xezim::simulate;

#[test]
fn struct_array_localparam_in_generate_conditions_below_the_top() {
    let got: Vec<String> = simulate(
        r#"
// unpacked struct-array localparams in a submodule: generate conditions on a
// member, and run-time reads of the elements
package q;
  typedef struct packed { logic [1:0] variant; logic [5:0] size; } pinfo_t;
endpackage
module ctrl (output logic [3:0] req, output logic [3:0] preq, output logic [1:0] v2, output logic [5:0] s3);
  typedef struct packed { logic [1:0] variant; logic [5:0] size; } info_t;
  localparam info_t Info [4] = '{'{2'd0, 6'd5}, '{2'd1, 6'd6}, '{2'd0, 6'd7}, '{2'd2, 6'd8}};
  localparam q::pinfo_t PInfo [4] = '{'{2'd1, 6'd1}, '{2'd0, 6'd2}, '{2'd1, 6'd3}, '{2'd0, 6'd4}};
  for (genvar k = 0; k < 4; k++) begin : gp
    if (Info[k].variant == 2'd1) begin : gb
      assign req[k] = 1'b1;
    end else if (Info[k].variant == 2'd2) begin : gl
      assign req[k] = 1'b1;
    end else begin : gu
      assign req[k] = 1'b0;
    end
    if (PInfo[k].variant == 2'd1) begin : pb
      assign preq[k] = 1'b1;
    end else begin : pu
      assign preq[k] = 1'b0;
    end
  end
  assign v2 = Info[2].variant ^ Info[3].variant;
  assign s3 = Info[3].size;
endmodule
module top;
  logic [3:0] req, preq; logic [1:0] v2; logic [5:0] s3;
  ctrl dut(.req(req), .preq(preq), .v2(v2), .s3(s3));
  initial begin #1 $display("g13: req=%b preq=%b v2=%0d s3=%0d", req, preq, v2, s3); end
endmodule
"#,
        100,
    )
    .expect("simulate failed")
    .output
    .iter()
    .map(|o| o.message.clone())
    .collect();
    let want = "g13: req=1010 preq=0101 v2=2 s3=8";
    assert!(
        got.iter().any(|l| l == want),
        "missing `{want}` in {got:#?}"
    );
}
