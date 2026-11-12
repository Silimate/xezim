// top: c14ew
`timescale 1ns/1ps
module c14ew;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [3:0] q = 4'b0000;
  clocking cb @(posedge clk);
    default output #2;
    output q;
  endclocking
  initial begin
    cb.q <= 4'b1010;
    @(cb);
    cb.q[0] <= 1'b1;
    @(cb);
  end
  initial begin
    #6 $display("T|a|t=%0t q=%b", $time, q);
    #2 $display("T|b|t=%0t q=%b", $time, q);
    $finish;
  end
endmodule
