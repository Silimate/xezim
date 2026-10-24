//! IEEE 1800-2017 §7.4.5, §7.6, §10.3: `assign dst = src[row];` drives a 1-D
//! unpacked array from one row of a 2-D unpacked array with the same inner
//! count, element by element, left bound to left bound. The row index may be
//! a literal or a localparam expression, and `dst` may be an output port.
//! The assign had no target and `dst` stayed unassigned.

use xezim::simulate;

const SRC: &str = r#"
module producer (output logic [7:0] row [3], output logic [7:0] prow [3]);
  localparam int ST = 2;
  logic [7:0] stage [2][3];
  logic [7:0] pst [ST][3];
  assign row = stage[1];
  assign prow = pst[ST-1];
  initial begin
    stage[1][0] = 8'h11; stage[1][1] = 8'h22; stage[1][2] = 8'h33;
    pst[1][0] = 8'h44; pst[1][1] = 8'h55; pst[1][2] = 8'h66;
  end
endmodule
module top;
  logic [7:0] got [3], pgot [3], local_row [3];
  logic [7:0] down_row [2:0];
  logic [7:0] local_stage [2][3];
  producer u (.row(got), .prow(pgot));
  assign local_row = local_stage[0];
  assign down_row = local_stage[0];
  initial begin
    local_stage[0][0] = 8'h77; local_stage[0][1] = 8'h88; local_stage[0][2] = 8'h99;
  end
  initial #1 $display("%h %h %h | %h %h %h | %h %h %h | %h %h %h", got[0], got[1], got[2],
                      pgot[0], pgot[1], pgot[2], local_row[0], local_row[1], local_row[2],
                      down_row[2], down_row[1], down_row[0]);
endmodule
"#;

#[test]
fn row_of_2d_array_assign() {
    let sim = simulate(SRC, 100).expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert_eq!(msgs, ["11 22 33 | 44 55 66 | 77 88 99 | 77 88 99"]);
}
