module top;
  class holder;
    event ce;
  endclass
  holder h;
  initial begin
    h = new();
    h.ce = null;
    h.ce = h.ce;
    if (h.ce == null)
      $display("T|15.5.5d|null after self assign");
    else
      $display("T|15.5.5d|not null after self assign");
  end
endmodule
