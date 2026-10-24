// A fetch driver drops ready after each handshake and, for a back-to-back
// fetch, raises it again in the same slot after a zero-cycle wait (fork,
// join_any, disable fork). Both drives belong to this event and mature at
// the negedge output skew; the last one wins, so ready stays high.
`timescale 1ns/1ps
module c14ln;
  logic clk = 0;
  always #5 clk = ~clk;
  logic ready;
  clocking cb @(posedge clk);
    default output negedge;
    output ready;
  endclocking
  task automatic wait_clks(int n);
    fork
      repeat (n) @(cb);
      #1000;
    join_any
    disable fork;
  endtask
  task automatic read_insn();
    wait_clks(0);
    cb.ready <= 1'b1;
    @(cb);
    cb.ready <= 1'b0;
  endtask
  initial begin
    @(cb);
    cb.ready <= 1'b0;
    @(cb);
    repeat (3) read_insn();
  end
  always @(posedge clk)
    $display("T|lastwins_neg|t=%0t ready=%b", $time, ready);
  initial begin
    repeat (6) @(posedge clk);
    #1 $finish;
  end
endmodule
