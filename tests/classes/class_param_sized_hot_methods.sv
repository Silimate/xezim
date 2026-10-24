// mem_model shape, called past the method compile threshold: formals typed
// by a class typedef over a class parameter keep the specialization width
// once the method runs compiled.
package bp;
  localparam int AW = 32;
endpackage

class mm #(int AddrWidth = bp::AW, int DataWidth = 32);
  localparam int MaskWidth = DataWidth / 8;
  typedef logic [AddrWidth-1:0] mem_addr_t;
  typedef logic [DataWidth-1:0] mem_data_t;
  typedef logic [MaskWidth-1:0] mem_mask_t;
  logic [7:0] system_memory[mem_addr_t];

  function void write_byte(mem_addr_t addr, logic [7:0] data);
    system_memory[addr] = data;
  endfunction

  function bit addr_exists(mem_addr_t addr);
    return system_memory.exists(addr);
  endfunction

  function void write(mem_addr_t addr, mem_data_t data, mem_mask_t mask = '1);
    for (int i = 0; i < DataWidth / 8; i++) begin
      if (mask[0]) write_byte(addr + i, data[7:0]);
      data = data >> 8;
      mask = mask >> 1;
    end
  endfunction
endclass

module top;
  mm m32;
  mm #(40) m40;
  int miss32, miss40;
  initial begin
    m32 = new;
    m40 = new;
    for (int i = 0; i < 3000; i++) begin
      m32.write_byte(32'h8000_0000 + i, i);
      m40.write_byte(40'h80_0000_0000 + i, i);
    end
    for (int i = 0; i < 3000; i++) begin
      if (!m32.addr_exists(32'h8000_0000 + i)) miss32++;
      if (!m40.addr_exists(40'h80_0000_0000 + i)) miss40++;
    end
    $display("T|m32|%0d %0d", m32.system_memory.size(), miss32);
    $display("T|m40|%0d %0d", m40.system_memory.size(), miss40);
    for (int i = 0; i < 1500; i++) m32.write(32'h9000_0000 + 4 * i, 32'h0403_0201, 4'b0101);
    $display("T|mask|%0d %h %h", m32.system_memory.size(),
             m32.system_memory[32'h9000_176c], m32.system_memory[32'h9000_176e]);
  end
endmodule
