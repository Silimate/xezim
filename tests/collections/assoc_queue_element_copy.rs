//! §7.6: `q = aq[k]`, where `aq` is an associative array of queues, resizes
//! `q` and copies the whole queue element, for int and string keys, inside a
//! method and through a handle.
#[test]
fn assoc_queue_element_copies_whole() {
    let result = xezim::simulate(
        r#"
class h_c;
  int by_key[string][$];
  function int fill();
    int mine[$];
    by_key["b"].push_back(3);
    by_key["b"].push_back(4);
    mine = by_key["b"];
    return mine.size();
  endfunction
endclass
module t;
  int mi[int][$];
  int ms[string][$];
  initial begin
    int j[$];
    int k[$];
    h_c h = new();
    mi[7].push_back(5);
    mi[7].push_back(6);
    j = mi[7];
    $display("int %0d %0d", j.size(), j[1]);
    ms["a"].push_back(8);
    ms["a"].push_back(9);
    ms["a"].push_back(10);
    j = ms["a"];
    $display("string %0d %0d", j.size(), j[2]);
    $display("method %0d", h.fill());
    k = h.by_key["b"];
    $display("handle %0d %0d", k.size(), k[1]);
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
    assert_eq!(lines, ["int 2 6", "string 3 10", "method 2", "handle 2 4"]);
}
