//! §12.7.3: `foreach (g.ph[i].ids[j])` iterates the dynamic array property
//! of element `i` with that element's own size (riscv-dv
//! `foreach (callstack_gen.program_h[i].sub_program_id[j])`), including an
//! empty one.
#[test]
fn foreach_over_member_array_of_indexed_element() {
    let result = xezim::simulate(
        r#"
typedef bit [15:0] pid_t;
class p_c;
  rand pid_t ids[];
endclass
class g_c;
  p_c ph[];
  function void init(int n);
    ph = new[n];
    foreach (ph[i]) ph[i] = new();
  endfunction
  function string own_walk();
    string s;
    foreach (ph[i]) foreach (ph[i].ids[j]) s = {s, $sformatf(" %0d:%0d", i, ph[i].ids[j])};
    return s;
  endfunction
endclass
class a_c;
  g_c g;
  function void run();
    string s;
    g = new();
    g.init(4);
    g.ph[0].ids = new[1];
    g.ph[0].ids[0] = 1;
    g.ph[1].ids = new[3];
    foreach (g.ph[1].ids[k]) g.ph[1].ids[k] = k + 2;
    foreach (g.ph[i]) begin
      foreach (g.ph[i].ids[j]) begin
        s = {s, $sformatf(" %0d:%0d", i, g.ph[i].ids[j])};
      end
    end
    $display("walk%s", s);
    $display("own%s", g.own_walk());
  endfunction
endclass
module t;
  initial begin
    a_c a = new();
    a.run();
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
    assert_eq!(lines, ["walk 0:1 1:2 1:3 1:4", "own 0:1 1:2 1:3 1:4"]);
}
