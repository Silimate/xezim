// top: c14ic
`timescale 1ns/1ps
module c14ic;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [3:0] o1 = 4'b1111, o2 = 4'b0000;
  clocking cb @(posedge clk);
    default output #2;
    output o1, o2;
  endclocking
  initial begin
    @(cb);
    #1;
    cb.o2[0] <= 1'b1;
    cb.o1 <= 4'b0000;
    cb.o2 <= 4'b1010;
    @(cb);
  end
  initial begin
    #16 $display("T|a|t=%0t o1=%b o2=%b", $time, o1, o2);
    #2 $display("T|b|t=%0t o1=%b o2=%b", $time, o1, o2);
    $finish;
  end
endmodule
