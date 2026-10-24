// §8.25: a method returning a class typedef over a class parameter must keep
// the specialization width once the method runs compiled (after 1000 calls).
class mem_model #(int AddrWidth = 32, int DataWidth = 32);
  typedef logic [AddrWidth-1:0] mem_addr_t;
  function mem_addr_t pass(mem_addr_t a);
    return a;
  endfunction
endclass

module top;
  mem_model m32;
  mem_model #(40) m40;
  int bad32, bad40;
  initial begin
    m32 = new;
    m40 = new;
    for (int i = 0; i < 1500; i++) begin
      if (m32.pass(32'h8000_0000 + i) !== 32'h8000_0000 + i) bad32++;
      if (m40.pass(40'h80_0000_0000 + i) !== 40'h80_0000_0000 + i) bad40++;
    end
    $display("T|hot_pass|%0d %0d", bad32, bad40);
  end
endmodule
