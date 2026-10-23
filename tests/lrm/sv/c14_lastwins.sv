// Two drives of one clockvar issued after @(cb) in the same slot are both
// this event's drives, and the last one wins. A response driver clears
// rvalid after each edge and sets it again when the next response is
// already queued, so back-to-back responses keep rvalid high.
`timescale 1ns/1ps
module c14l;
  logic clk = 0;
  always #5 clk = ~clk;
  logic rvalid = 0;
  clocking cb @(posedge clk);
    output rvalid;
  endclocking
  mailbox #(int) q = new();
  initial begin
    @(cb);
    q.put(1);
    @(cb);
    q.put(2);
  end
  initial begin
    int tr;
    forever begin
      @(cb);
      cb.rvalid <= 1'b0;
      q.get(tr);
      cb.rvalid <= 1'b1;
    end
  end
  always @(posedge clk)
    $display("T|lastwins|t=%0t rvalid=%b", $time, rvalid);
  initial begin
    repeat (4) @(posedge clk);
    #1 $finish;
  end
endmodule
