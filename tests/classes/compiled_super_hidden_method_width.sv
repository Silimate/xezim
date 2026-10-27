// IEEE 1800-2023 §13.5.1 / §11.6.1: `super.m(...)` calls the parent's `m`, so
// each actual takes the context width of the PARENT's formal, also when the
// derived class hides `m` with a wider formal. With 8-bit a = 200, b = 100,
// `(a + b) >> 1` in an 8-bit context is 22, not 150.
class P;
  function int m(logic [7:0] v);
    return v;
  endfunction
endclass

class D extends P;
  function int m(logic [31:0] v);
    return v;
  endfunction
  function int probe(logic [7:0] a, logic [7:0] b);
    return super.m((a + b) >> 1);
  endfunction
endclass

module top;
  initial begin
    D d = new;
    int r;
    int bad = 0;
    for (int i = 0; i < 1500; i++) begin
      r = d.probe(8'd200, 8'd100);
      if (r != 22) bad++;
    end
    if (bad == 0) $display("TAG_PASS");
    else $display("TAG_FAIL r=%0d bad=%0d", r, bad);
    $finish;
  end
endmodule
