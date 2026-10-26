//! §12.7.3: `foreach (g.ph[1].ids[j])` iterates the dynamic array property
//! of that one element of an array of handles, with that element's own size,
//! including an empty one.
#[test]
fn foreach_over_member_array_of_indexed_element() {
    let result = xezim::simulate(
        r#"
class p_c;
  int ids[];
endclass
class g_c;
  p_c ph[];
endclass
class a_c;
  g_c g;
  function void run();
    string s;
    g = new();
    g.ph = new[3];
    foreach (g.ph[i]) g.ph[i] = new();
    g.ph[0].ids = new[1];
    g.ph[0].ids[0] = 1;
    g.ph[1].ids = new[3];
    g.ph[1].ids[0] = 2;
    g.ph[1].ids[1] = 3;
    g.ph[1].ids[2] = 4;
    foreach (g.ph[0].ids[j]) s = {s, $sformatf(" 0:%0d", g.ph[0].ids[j])};
    foreach (g.ph[1].ids[j]) s = {s, $sformatf(" 1:%0d", g.ph[1].ids[j])};
    foreach (g.ph[2].ids[j]) s = {s, $sformatf(" 2:%0d", g.ph[2].ids[j])};
    $display("walk%s", s);
  endfunction
endclass
module t;
  a_c a;
  initial begin
    a = new();
    a.run();
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
    assert_eq!(lines, ["walk 0:1 1:2 1:3 1:4"]);
}
