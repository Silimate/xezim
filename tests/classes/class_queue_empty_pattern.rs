//! `q = {}` on a class queue replaces the queue with an empty one.
#[test]
fn class_queue_empty_pattern_clears() {
    let result = xezim::simulate(
        r#"
class b;
  string q[$] = {"base"};
endclass
class d extends b;
  function void clear();
    q = {};
  endfunction
  function void clear_pat();
    q = '{};
  endfunction
endclass
module t;
  string mq[$] = {"m"};
  initial begin
    d obj = new();
    d obj2 = new();
    obj.clear();
    obj2.clear_pat();
    mq = {};
    $display("class %0d", obj.q.size());
    $display("pat %0d", obj2.q.size());
    $display("mod %0d", mq.size());
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
    assert!(
        lines.contains(&"class 0") && lines.contains(&"pat 0") && lines.contains(&"mod 0"),
        "{lines:?}"
    );
}
