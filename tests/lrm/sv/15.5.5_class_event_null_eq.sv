module top;
  class holder;
    event ce;
  endclass
  holder h1, h2;
  initial begin
    h1 = new();
    h2 = new();
    h1.ce = null;
    h2.ce = null;
    if (h1.ce == h2.ce)
      $display("T|15.5.5c|independent null events equal");
    else
      $display("T|15.5.5c|independent null events differ");
  end
endmodule
