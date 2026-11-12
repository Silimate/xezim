// top: c14kb
`timescale 1ns/1ps
module c14kb;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [3:0] q = 4'b0000;
  clocking cb @(posedge clk);
    default output #3;
    output q;
  endclocking
  initial begin
    @(cb);
    cb.q[0] <= 1'b1;
    #1 q[3] = 1'b1;
    @(cb);
  end
  initial begin
    #7 $display("T|a|t=%0t q=%b", $time, q);
    #2 $display("T|b|t=%0t q=%b", $time, q);
    $finish;
  end
endmodule
