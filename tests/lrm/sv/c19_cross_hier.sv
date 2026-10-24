// IEEE 1800-2017 §19.6, extended to hierarchical variable references as
// Questa also accepts: crossing them, each with an implicit coverpoint, from
// an interface bound into the module that holds them.
module stage (input logic a, input logic b);
endmodule
module regs;
  typedef struct packed { logic ie; logic [1:0] pp; } status_t;
  status_t state_q;
endmodule
module core (input logic clk, input logic a, input logic b, input logic ie);
  stage stage_i (.a(a), .b(b));
  regs regs_i ();
  assign regs_i.state_q.ie = ie;
  assign regs_i.state_q.pp = 2'b11;
endmodule
interface cov_if (input logic clk);
  covergroup cg @(posedge clk);
    cp_a: coverpoint stage_i.a;
    ab_cross: cross stage_i.a, stage_i.b;
    ie_cross: cross cp_a, regs_i.state_q.ie iff (stage_i.b);
  endgroup
  cg cg_i = new;
  initial begin
    #20;
    $display("T|ab|%0.2f", cg_i.ab_cross.get_inst_coverage());
    $display("T|ie|%0.2f", cg_i.ie_cross.get_inst_coverage());
    $display("T|cg|%0.2f", cg_i.get_inst_coverage());
    $finish;
  end
endinterface
module c19_cross_hier;
  logic clk = 0, a = 0, b = 0, ie = 0;
  core u_core (.clk(clk), .a(a), .b(b), .ie(ie));
  bind core cov_if u_cov (.clk(clk));
  initial begin
    // (a,b,ie): 00x, 01 0, 10x, 11 1 -> ab_cross gets all 4 combos;
    // ie_cross samples only when b=1: (a=0,ie=0) and (a=1,ie=1) -> 2 of 4.
    #1 a = 0; b = 0; ie = 0; #1 clk = 1; #1 clk = 0;
    #1 a = 0; b = 1; ie = 0; #1 clk = 1; #1 clk = 0;
    #1 a = 1; b = 0; ie = 1; #1 clk = 1; #1 clk = 0;
    #1 a = 1; b = 1; ie = 1; #1 clk = 1; #1 clk = 0;
  end
endmodule
