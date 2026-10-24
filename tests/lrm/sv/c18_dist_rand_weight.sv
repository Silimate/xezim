// A dist weight that reads other rand variables is the weight of the
// solution drawn: `[1:$] :/ (inv ? 1000 : en ? 0 : 1)` never gives a nonzero
// seed with `en && !inv`, also when another dist or an implication joins the
// solve. A variable drawn by such a dist in every case does not outweigh the
// values of the variables its weight reads.
class c18dw_item;
  rand bit        en;
  rand bit        inv;
  rand bit [31:0] seed;
  constraint c_seed {
    seed dist { 0 :/ 1, [1:32'hffffffff] :/ (inv ? 1000 : en ? 0 : 1) };
  }
endclass

module c18dw;
  initial begin
    c18dw_item r = new();
    bit stale = 0;
    int bad = 0, en_only = 0, dis_seed = 0, dis = 0;
    repeat (400) begin
      void'(r.randomize() with { inv dist {1'b0 :/ 49, 1'b1 :/ 1}; });
      if (r.en && !r.inv && r.seed != 0) bad++;
      if (!r.en && !r.inv) begin
        dis++;
        if (r.seed != 0) dis_seed++;
      end
      void'(r.randomize() with { stale && en -> inv; !(stale && en) -> !inv; });
      if (r.en && !r.inv && r.seed != 0) bad++;
      if (!stale && r.en) en_only++;
      stale = $urandom_range(1);
    end
    $display("T|dist_rand_weight|bad=%0d", bad);
    $display("T|dist_rand_weight|disabled draws a seed some=%0d", dis_seed > dis / 4 && dis_seed < dis * 3 / 4);
    $display("T|dist_rand_weight|enabled without stale some=%0d", en_only > 40);
    $finish;
  end
endmodule
