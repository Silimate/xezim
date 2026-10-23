// A clocking drive set before @(cb) and cleared after it must stay high for
// one cycle. The clear is the next event's output, not a replacement of the
// value already queued for this event.
`timescale 1ns/1ps
module c14p;
  logic clk = 0;
  always #5 clk = ~clk;
  logic branch = 0, en = 0;
  logic [7:0] addr = 0;
  clocking cb @(posedge clk);
    default output #1;
    output branch, addr, en;
  endclocking
  task automatic pulse(logic [7:0] a);
    cb.branch <= 1'b1;
    cb.addr <= a;
    cb.en <= 1'b1;
    @(cb);
    cb.branch <= 1'b0;
    cb.addr <= 8'hxx;
  endtask
  initial begin
    #1 pulse(8'h2a);
    @(posedge clk);
    @(posedge clk);
    #1 $finish;
  end
  always @(posedge clk)
    $display("T|pulse|t=%0t br=%b addr=%h en=%b", $time, branch, addr, en);
endmodule
