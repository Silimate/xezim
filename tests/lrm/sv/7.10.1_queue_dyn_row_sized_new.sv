// §7.10.1 / §7.5.1: `q[0] = new[n]` on an empty queue of dynamic-array rows grows the queue.
module rqdr;
  int q[$][];
  initial begin
    q[0] = new[2];
    $display("T|7.10.1qrow|%0d %0d", q.size(), q[0].size());
  end
endmodule
