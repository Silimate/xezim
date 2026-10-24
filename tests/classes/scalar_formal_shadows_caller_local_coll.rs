//! §13.3, §23.9: a scalar formal or local of a callee names its own variable,
//! not a caller's same-named local queue. riscv-dv calls `uvm_split_string`
//! (formal `string str`) under `pre_enter_privileged_mode`, whose local
//! `string str[$]` made the split return one empty field.
#[test]
fn scalar_formal_and_local_shadow_caller_local_queue() {
    let result = xezim::simulate(
        r#"
function automatic void split(string str, byte sep, ref string values[$]);
  int s = 0, e = 0;
  values.delete();
  while (e < str.len()) begin
    for (s = e; e < str.len(); ++e)
      if (str[e] == sep) break;
    if (s != e) values.push_back(str.substr(s, e - 1));
    e++;
  end
endfunction
class cfg;
  function string parse(string arg);
    string fields[$];
    split(arg, ",", fields);
    return $sformatf("%0d %p", fields.size(), fields);
  endfunction
  function int count();
    string str;
    str = "abcde";
    return str.len();
  endfunction
  task tparse(string str, output int n);
    n = str.len();
  endtask
endclass
class gen;
  cfg c = new();
  function void outer();
    string str[$];
    int n;
    str = {"la x4, kernel_stack_end", "x"};
    $display("split %s", c.parse("MML:0,MMWP:0,RLB:0"));
    $display("local %0d", c.count());
    c.tparse("abc", n);
    $display("task %0d", n);
    $display("own %0d", str.size());
  endfunction
endclass
module t;
  initial begin
    gen g = new();
    g.outer();
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
    assert_eq!(
        lines,
        [
            r#"split 3 '{"MML:0", "MMWP:0", "RLB:0"}"#,
            "local 5",
            "task 3",
            "own 2"
        ]
    );
}
