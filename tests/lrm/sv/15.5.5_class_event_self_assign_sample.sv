module top;
  class holder;
    bit v;
    event ce;
    covergroup cg @(ce);
      option.at_least = 2;
      cp: coverpoint v { bins hit = {1}; }
    endgroup
    function new();
      cg = new();
    endfunction
  endclass
  holder h;
  initial begin
    h = new();
    h.ce = h.ce;
    h.v = 1;
    #1 -> h.ce;
    #1 if (h.cg.get_inst_coverage() == 0.0)
      $display("T|15.5.5e|one sample after self assign");
    else
      $display("T|15.5.5e|double sample after self assign");
  end
endmodule
