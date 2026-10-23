// §8.25, §6.18: inside a parameterized class, a method's formals, locals,
// and an associative array key typed by a class typedef or a packed range
// over a class parameter or localparam have the widths of the object's
// specialization.
class mem_model #(int AddrWidth = 32, int DataWidth = 32);
  localparam int MaskWidth = DataWidth / 8;
  typedef logic [AddrWidth-1:0] mem_addr_t;
  typedef logic [DataWidth-1:0] mem_data_t;
  typedef logic [MaskWidth-1:0] mem_mask_t;
  logic [7:0] system_memory[mem_addr_t];

  function void write(input mem_addr_t addr, mem_data_t data, mem_mask_t mask = '1);
    for (int i = 0; i < DataWidth / 8; i++) begin
      if (mask[0]) system_memory[addr + i] = data[7:0];
      data = data >> 8;
      mask = mask >> 1;
    end
  endfunction

  function mem_data_t read(mem_addr_t addr, mem_mask_t mask = '1);
    mem_data_t data;
    for (int i = DataWidth / 8 - 1; i >= 0; i--) begin
      data = data << 8;
      if (mask[MaskWidth - 1]) data[7:0] = system_memory[addr + i];
      mask = mask << 1;
    end
    return data;
  endfunction

  function void widths(string tag, mem_mask_t m, logic [MaskWidth-1:0] dm, mem_data_t d);
    mem_mask_t lm;
    logic [MaskWidth-1:0] ldm;
    mem_data_t ld;
    logic [DataWidth-1:0] ldd;
    $display("T|%s_formal|%0d %0d %0d", tag, $bits(m), $bits(dm), $bits(d));
    $display("T|%s_local|%0d %0d %0d %0d", tag, $bits(lm), $bits(ldm), $bits(ld), $bits(ldd));
    $display("T|%s_formal_val|%0h %0h", tag, m, dm);
  endfunction
endclass

class keyed #(int AW = 32);
  typedef logic [AW-1:0] a_t;
  logic [7:0] m[a_t];
  function void w(a_t a); m[a] = 8'h5a; m[a + 1] = 8'h5b; endfunction
endclass

module top;
  mem_model m32;
  mem_model #(32, 64) m64;
  keyed kd;
  keyed #(8) k8;
  keyed #(40) k40;
  logic [31:0] r;
  initial begin
    m32 = new;
    m64 = new;
    m32.write(32'h8000_0000, 32'h1234_5678);
    r = m32.read(32'h8000_0000);
    $display("T|mem|%0d %h", m32.system_memory.size(), r);
    m32.widths("d32", 16'hffff, 16'hffff, 0);
    m64.widths("d64", 16'hffff, 16'hffff, 0);
    kd = new;
    k8 = new;
    k40 = new;
    kd.w(32'hff);
    k8.w(8'hff);
    k40.w(40'hff_ffff_ffff);
    foreach (kd.m[k]) $display("T|kd_key|%0h", k);
    foreach (k8.m[k]) $display("T|k8_key|%0h", k);
    foreach (k40.m[k]) $display("T|k40_key|%0h", k);
  end
endmodule
