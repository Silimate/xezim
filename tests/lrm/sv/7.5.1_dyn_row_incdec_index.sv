// §7.5.1 / §11.4.1: `d[i++] = new[n]` sizes row `d[0]` once; index side effect runs once.
module rdx;
  int i;
  bit d[][];
  initial begin
    d = new[3];
    i = 0;
    d[i++] = new[2];
    $display("T|7.5.1row_inc|%0d %0d %0d", i, d[0].size(), d[1].size());
  end
endmodule
