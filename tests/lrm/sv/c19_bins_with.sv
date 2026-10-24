// §19.5.1.1: bins with a `with` filter keep only the values the filter
// accepts; a fixed-size array bin spreads those values, in order, over its
// bins (§19.5.1).
package mubi_pkg;
  parameter logic [31:0] MuBi32True = 32'h9696_9696;
  parameter logic [31:0] MuBi32False = 32'h6969_6969;
endpackage
class mubi_cov #(int Width = 4, logic [Width-1:0] ValueTrue = 4'h6,
                 logic [Width-1:0] ValueFalse = 4'h9);
  logic [Width-1:0] value;
  covergroup mubi_cg;
    cp_value: coverpoint value {
      bins true = {ValueTrue};
      bins false = {ValueFalse};
      bins others[Width] = {[0:{Width{1'b1}}]} with (!(item inside {ValueTrue, ValueFalse}));
    }
  endgroup
  function new(); mubi_cg = new; endfunction
  function void sample(logic [Width-1:0] v); value = v; mubi_cg.sample(); endfunction
endclass
class mubi32_cov;
  logic [31:0] value;
  covergroup mubi_cg;
    cp_value: coverpoint value {
      bins true = {mubi_pkg::MuBi32True};
      bins false = {mubi_pkg::MuBi32False};
      bins others[32] = {[0:{32{1'b1}}]} with (!(item inside {mubi_pkg::MuBi32True,
                                                              mubi_pkg::MuBi32False}));
    }
  endgroup
  function new(); mubi_cg = new; endfunction
  function void sample(logic [31:0] v); value = v; mubi_cg.sample(); endfunction
endclass
module c19_bins_with;
  logic [3:0] v;
  covergroup even_cg;
    coverpoint v { bins even[] = {[0:15]} with (item % 2 == 0); }
  endgroup
  covergroup odd_cg;
    coverpoint v {
      bins odd = {[0:7]} with (item[0]);
      bins big = {[8:15]};
    }
  endgroup
  mubi_cov m4;
  mubi32_cov m32;
  even_cg ec;
  odd_cg oc;
  initial begin
    m4 = new;
    m4.sample(4'h6); m4.sample(4'h9); m4.sample(4'h2); m4.sample(4'h5);
    m4.sample(4'ha); m4.sample(4'hb);
    $display("T|mubi4|%0.2f", m4.mubi_cg.get_inst_coverage());
    m32 = new;
    m32.sample(mubi_pkg::MuBi32True); m32.sample(mubi_pkg::MuBi32False);
    m32.sample(0); m32.sample(32'hffff_ffff);
    $display("T|mubi32|%0.2f", m32.mubi_cg.get_inst_coverage());
    ec = new;
    v = 2; ec.sample(); v = 3; ec.sample();
    $display("T|even|%0.2f", ec.get_inst_coverage());
    oc = new;
    v = 2; oc.sample();
    $display("T|odd_2|%0.2f", oc.get_inst_coverage());
    v = 3; oc.sample();
    $display("T|odd_3|%0.2f", oc.get_inst_coverage());
  end
endmodule
