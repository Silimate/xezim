//! §18.3/§18.4: every element of a rand dynamic array or queue is a random
//! variable, and one of enum type takes only the enum's named constants.
//! The elements were drawn as any bit pattern of the enum's width.
#[test]
fn rand_enum_collection_elements_take_enum_values() {
    let result = xezim::simulate(
        r#"
typedef enum bit [2:0] { M_A = 1, M_B = 5 } mode_t;
class c;
  rand mode_t d[];
  rand mode_t s[];
  rand mode_t q[$];
  rand mode_t n[];
  rand mode_t p[];
  rand mode_t m[];
  constraint k {
    s.size() == 4;
    q.size() == 4;
    foreach (n[i]) n[i] != M_A;
    p.size() == 2;
    p[0] != p[1];
    m.size() == 16;
    m.sum() with (int'(item)) == 76;
  }
  function new(); d = new[4]; n = new[4]; endfunction
endclass
module t;
  function automatic int bad(mode_t v[$]);
    foreach (v[i]) bad += !(v[i] inside {M_A, M_B});
  endfunction
  initial begin
    c o = new();
    int ok = 0, bd = 0, bs = 0, bq = 0, bn = 0, bp = 0, bm = 0;
    repeat (10) if (o.randomize()) begin
      ok++;
      bd += bad(o.d);
      bs += bad(o.s);
      bq += bad(o.q);
      foreach (o.n[i]) bn += o.n[i] != M_B;
      bp += bad(o.p) + (o.p[0] == o.p[1]);
      bm += bad(o.m);
    end
    $display("ok %0d sizes %0d %0d %0d %0d %0d %0d", ok,
             o.d.size(), o.s.size(), o.q.size(), o.n.size(), o.p.size(), o.m.size());
    $display("bad %0d %0d %0d %0d %0d %0d", bd, bs, bq, bn, bp, bm);
    $finish;
  end
endmodule
"#,
        100_000,
    )
    .unwrap();
    let lines: Vec<_> = result
        .output
        .iter()
        .map(|entry| entry.message.as_str())
        .collect();
    assert_eq!(lines, ["ok 10 sizes 4 4 4 4 2 16", "bad 0 0 0 0 0 0"]);
}
