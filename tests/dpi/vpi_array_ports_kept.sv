// Unpacked-array ports that keep storage of their own, beside one that
// collapses onto its actual, for tests/dpi/vpi_array_ports_kept.c (see
// tests/strings/vpi_array_ports.rs).
module two (input logic clk, input bit [7:0] a [0:1], output bit [7:0] q);
  always_ff @(posedge clk) q <= a[0];
endmodule

module four (input logic clk, input logic [7:0] a [0:1], output logic [7:0] q);
  always_ff @(posedge clk) q <= a[0];
endmodule

module top (input logic [7:0] p [0:1], input logic [7:0] e [0:1],
            input logic [7:0] k [0:1]);
  logic clk = 0;
  always #5 clk = ~clk;
  bit [7:0] qp;
  logic [7:0] qe, qk;
  // A two-state port on a four-state actual, a connection §7.6 does not
  // allow and xezim accepts: each array keeps values of its own type.
  two  up (.clk(clk), .a(p), .q(qp));
  // A second continuous driver on an element of the port: the drivers stay
  // on the names they were elaborated for.
  four ue (.clk(clk), .a(e), .q(qe));
  assign ue.a[1] = 8'h55;
  // Collapses.
  four uk (.clk(clk), .a(k), .q(qk));
  initial begin
    #7;
    $display("T|kept|%0d %0d", qe, qk);
    $finish;
  end
endmodule
