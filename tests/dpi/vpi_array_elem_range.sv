// Unpacked-array ports of the top module, driven only by the VPI library
// tests/dpi/vpi_array_elem_range.c (see tests/strings/vpi_array_elem_range.rs):
// the shape of the I3C core's `priv_ids_i`, a descending array of ascending
// vectors, and a two-dimensional array.
package vpi_range_pkg;
  parameter int Count = 2;
endpackage

module vpi_range_probe #(parameter int Count = 3) ();
  for (genvar i = vpi_range_pkg::Count; i < Count; ++i) begin : row
    logic [Count+i-1:0] words [vpi_range_pkg::Count];
  end
endmodule

module top (
  input  logic [31:0] ids [4],
  input  logic [0:7]  down [3:0],
  input  logic [15:8] grid [0:1][2:0],
  output logic [31:0] sum
);
  vpi_range_probe #(3) short_probe();
  vpi_range_probe #(4) long_probe();
  always_comb begin
    sum = '0;
    for (int i = 0; i < 4; i++) sum += ids[i];
  end
  initial #10 $finish;
endmodule
