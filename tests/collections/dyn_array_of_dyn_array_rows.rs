//! §7.5 / §7.5.1: `d[i] = new[n]` sizes row `d[i]` of a dynamic array of
//! dynamic arrays, for a class property and for a local, and `d[i][j] = v`
//! then writes one element of that row. Sizing a row does not construct the
//! enclosing class, and an element of a class-handle array still constructs.
#[test]
fn dyn_array_of_dyn_array_rows_size_and_write() {
    let result = xezim::simulate(
        r#"
class item;
  int n;
  function new(int n); this.n = n; endfunction
endclass
class c;
  static int ctor_calls;
  bit v[][];
  item objs[];
  function new();
    ctor_calls++;
  endfunction
  function void build(int n);
    v = new[2];
    foreach (v[i]) v[i] = new[n];
    v[1][n - 1] = 1;
    objs = new[2];
    foreach (objs[i]) objs[i] = new(i + 7);
  endfunction
endclass
module t;
  c o;
  int bad;
  task automatic local_rows();
    bit w[][];
    w = new[3];
    foreach (w[i]) w[i] = new[i + 2];
    $display("local %0d %0d %0d %0d", w.size(), w[0].size(), w[1].size(), w[2].size());
  endtask
  initial begin
    o = new();
    for (int k = 1; k <= 200; k++) begin
      o.build(k);
      bad += o.v[0].size() != k || o.v[1].size() != k || o.v[1][k - 1] != 1 || o.v[0][k - 1] != 0;
    end
    $display("ctor %0d bad %0d rows %0d %0d objs %0d %0d", c::ctor_calls, bad, o.v.size(),
             o.v[1].size(), o.objs[0].n, o.objs[1].n);
    local_rows();
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
    assert_eq!(
        lines,
        ["ctor 1 bad 0 rows 2 200 objs 7 8", "local 3 2 3 4"]
    );
}
