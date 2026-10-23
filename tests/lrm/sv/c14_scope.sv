// A deferred clocking drive waits for the next event, and so does a later
// drive by that process or by the parent a join resumes. A different process
// on the same block stays on this event.
`timescale 1ns/1ps
module c14s;
  logic clk = 0;
  always #5 clk = ~clk;

  logic br = 0, en = 0;
  logic [7:0] adr = 0;
  clocking cb1 @(posedge clk);
    default output #1;
    output br, adr, en;
  endclocking
  initial begin
    #1;
    cb1.br <= 1'b1;
    cb1.adr <= 8'h2a;
    @(cb1);
    cb1.br <= 1'b0;
    cb1.adr <= 8'hxx;
    cb1.en <= 1'b1;
  end

  logic pulse = 0, gnt = 0;
  event handoff;
  clocking cb2 @(posedge clk);
    default output #1;
    output pulse, gnt;
  endclocking
  initial begin
    #1;
    cb2.pulse <= 1'b1;
    @(cb2);
    cb2.pulse <= 1'b0;
    -> handoff;
  end
  initial begin
    @(handoff);
    cb2.gnt <= 1'b1;
  end

  logic req = 0, ready = 0;
  clocking cb3 @(posedge clk);
    default output #1;
    output req, ready;
  endclocking
  initial begin
    #1;
    cb3.req <= 1'b0;
    fork
      begin
        @(cb3);
        cb3.req <= 1'b1;
      end
    join
    cb3.ready <= 1'b1;
  end

  always @(posedge clk) begin
    $display("T|same|t=%0t br=%b adr=%h en=%b", $time, br, adr, en);
    $display("T|two|t=%0t pulse=%b gnt=%b", $time, pulse, gnt);
    $display("T|fork|t=%0t req=%b ready=%b", $time, req, ready);
  end
  initial begin
    @(posedge clk);
    @(posedge clk);
    @(posedge clk);
    #1 $finish;
  end
endmodule
