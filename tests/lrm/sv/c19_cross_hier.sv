// §19.6: crossing hierarchical variable references, each with an implicit
// coverpoint, from an interface bound into the module that holds them
// (the lowRISC core_ibex_fcov_if shape).
module id_stage (input logic a, input logic b);
endmodule
module cs_regs;
  typedef struct packed { logic mie; logic [1:0] mpp; } status_t;
  status_t mstatus_q;
endmodule
module core (input logic clk, input logic a, input logic b, input logic mie, input logic pend);
  id_stage id_stage_i (.a(a), .b(b));
  cs_regs cs_registers_i ();
  assign cs_registers_i.mstatus_q.mie = mie;
  assign cs_registers_i.mstatus_q.mpp = 2'b11;
endmodule
interface fcov_if (input logic clk);
  covergroup cg @(posedge clk);
    cp_a: coverpoint id_stage_i.a;
    ab_cross: cross id_stage_i.a, id_stage_i.b;
    mie_cross: cross cp_a, cs_registers_i.mstatus_q.mie iff (id_stage_i.b);
    // §19.6.1.1: binsof over a dotted item path, two and three segments deep.
    ign_cross: cross id_stage_i.a, cs_registers_i.mstatus_q.mie iff (id_stage_i.b) {
      ignore_bins a0_mie1 = binsof(id_stage_i.a) intersect {0} &&
                            binsof(cs_registers_i.mstatus_q.mie) intersect {1};
    }
  endgroup
  cg cg_i = new;
  initial begin
    #20;
    $display("T|ab|%0.2f", cg_i.ab_cross.get_inst_coverage());
    $display("T|mie|%0.2f", cg_i.mie_cross.get_inst_coverage());
    $display("T|ign|%0.2f", cg_i.ign_cross.get_inst_coverage());
    $display("T|cg|%0.2f", cg_i.get_inst_coverage());
    $finish;
  end
endinterface
module c19_cross_hier;
  logic clk = 0, a = 0, b = 0, mie = 0;
  core u_core (.clk(clk), .a(a), .b(b), .mie(mie), .pend(1'b0));
  bind core fcov_if u_fcov (.clk(clk));
  initial begin
    // (a,b,mie): 00x, 01 0, 10x, 11 1 -> ab_cross gets all 4 combos;
    // mie_cross samples only when b=1: (a=0,mie=0) and (a=1,mie=1) -> 2 of 4.
    // ign_cross drops the unhit (a=0,mie=1) -> 2 of 3.
    #1 a = 0; b = 0; mie = 0; #1 clk = 1; #1 clk = 0;
    #1 a = 0; b = 1; mie = 0; #1 clk = 1; #1 clk = 0;
    #1 a = 1; b = 0; mie = 1; #1 clk = 1; #1 clk = 0;
    #1 a = 1; b = 1; mie = 1; #1 clk = 1; #1 clk = 0;
  end
endmodule
