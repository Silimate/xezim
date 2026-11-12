// top: c14mca
`timescale 1ns/1ps
module c14mca;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [3:0] q;
  logic [3:0] d = 4'b0000;
  assign #1 q = d;
  clocking cb @(posedge clk);
    default output #2;
    output q;
  endclocking
  initial begin
    @(cb);
    cb.q[0] <= 1'b1;
    #1 d <= 4'b1110;
    @(cb);
  end
  initial begin
    #7 $display("T|a|t=%0t q=%b", $time, q);
    #2 $display("T|b|t=%0t q=%b", $time, q);
    $finish;
  end
endmodule
