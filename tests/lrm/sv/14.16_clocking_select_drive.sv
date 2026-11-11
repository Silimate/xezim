// top: csel
`timescale 1ns/1ps
module csel;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [3:0] nib = 0, o1 = 0, o2 = 0;
  logic [0:3] asc = 0;
  clocking cb @(posedge clk);
    default output #2;
    output nib, asc, o1, o2;
  endclocking
  initial begin
    @(cb); @(cb);
    cb.nib[0] <= 1'b1;
    cb.nib[2:1] <= 2'b11;
    cb.asc[0:1] <= 2'b11;
    cb.o1 <= 4'b1010;
    cb.o1[0] <= 1'b1;
    cb.o2[0] <= 1'b1;
    cb.o2 <= 4'b1010;
    #1 cb.nib[3] <= 1'b1;
    @(cb);
    cb.o1[2] <= ##1 1'b1;
  end
  initial begin
    #16 $display("T|a|t=%0t nib=%b asc=%b o1=%b o2=%b", $time, nib, asc, o1, o2);
    #2 $display("T|b|t=%0t nib=%b asc=%b o1=%b o2=%b", $time, nib, asc, o1, o2);
    #8 $display("T|c|t=%0t nib=%b", $time, nib);
    #2 $display("T|d|t=%0t nib=%b", $time, nib);
    #8 $display("T|e|t=%0t o1=%b", $time, o1);
    #2 $display("T|f|t=%0t o1=%b", $time, o1);
    $finish;
  end
endmodule
