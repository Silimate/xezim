//! §13.5 / §13.5.2: a collection member of an element of a subroutine-local
//! array or of a `ref` array formal (`lq[i].ds`, `sp[i].ds`) passed to an
//! `output` dynamic-array formal receives the callee's result. The formal
//! shadows a same-named class property, which keeps its own contents.
#[test]
fn output_formal_bound_to_local_array_elem_member() {
    let result = xezim::simulate(
        r#"
class item_c;
  int v;
  function new(int x); v = x; endfunction
endclass
class seq_c;
  item_c ds[];
endclass
class gen_c;
  string ds[$];
  function void gen_dir(input int n, output item_c ds[]);
    item_c it;
    for (int i = 0; i < n; i++) begin
      it = new(i + 10);
      ds = {ds, it};
    end
    ds.shuffle();
  endfunction
  function void fill(ref seq_c sp[]);
    sp = new[2];
    foreach (sp[i]) begin
      sp[i] = new();
      gen_dir(.n(3), .ds(sp[i].ds));
    end
  endfunction
  function void run();
    seq_c lq[];
    seq_c rq[];
    ds.push_back(".text");
    lq = new[2];
    foreach (lq[i]) begin
      lq[i] = new();
      gen_dir(2, lq[i].ds);
    end
    fill(rq);
    $display("local %0d %0d", lq[1].ds.size(), lq[1].ds[0].v + lq[1].ds[1].v);
    $display("ref %0d %0d", rq[1].ds.size(),
             rq[1].ds[0].v + rq[1].ds[1].v + rq[1].ds[2].v);
    $display("prop %0d %s", ds.size(), ds[0]);
  endfunction
endclass
module t;
  initial begin
    gen_c g = new();
    g.run();
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
    assert_eq!(lines, ["local 2 21", "ref 3 33", "prop 1 .text"]);
}
