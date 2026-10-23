// Two drives after @(cb) stay on one edge. The values queued before the
// wait occupy the edge that wakes the process; req and ready update together
// on the following edge.
`timescale 1ns/1ps
module c14q;
  logic clk = 0;
  always #5 clk = ~clk;
  logic req = 0, ready = 0;
  clocking cb @(posedge clk);
    default output #1;
    output req, ready;
  endclocking
  initial begin
    #1;
    cb.req <= 1'b0;
    cb.ready <= 1'b0;
    @(cb);
    cb.req <= 1'b1;
    cb.ready <= 1'b1;
    @(posedge clk);
    @(posedge clk);
    #1 $finish;
  end
  always @(posedge clk)
    $display("T|q|t=%0t req=%b ready=%b", $time, req, ready);
endmodule
