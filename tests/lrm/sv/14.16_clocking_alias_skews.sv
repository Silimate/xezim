// top: c14as
`timescale 1ns/1ps
module c14as;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [1:0] q = 2'b00;
  clocking cb @(posedge clk);
    output #2 a = q;
    output #4 b = q;
  endclocking
  initial begin
    @(cb);
    cb.b[0] <= 1'b1;
    cb.a[1] <= 1'b1;
    @(cb);
  end
  initial begin
    #6 $display("T|a|t=%0t q=%b", $time, q);
    #2 $display("T|b|t=%0t q=%b", $time, q);
    #2 $display("T|c|t=%0t q=%b", $time, q);
    $finish;
  end
endmodule
