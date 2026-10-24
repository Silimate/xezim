//! §13.5.2 / §7.4.5: a `ref` dynamic-array formal bound to one row of a
//! class property that is a fixed array of dynamic arrays
//! (`seq_c sub_program[1][]`, passed as `sub_program[0]`) aliases that row:
//! sizing and filling it in the callee is visible in the caller.
#[test]
fn ref_formal_bound_to_array_of_coll_row() {
    let result = xezim::simulate(
        r#"
class seq_c;
  string nm;
  function new(string n = "");
    nm = n;
  endfunction
endclass
class gen_c;
  seq_c sub_program[2][];
  function void gen_sub(ref seq_c sp[], input int n);
    sp = new[n];
    foreach (sp[i]) begin
      sp[i] = new($sformatf("sub_%0d", i + 1));
    end
    $display("callee %0d %s", sp.size(), sp[n - 1].nm);
  endfunction
  function void run();
    gen_sub(sub_program[1], 3);
  endfunction
endclass
module t;
  initial begin
    gen_c g = new();
    g.run();
    $display("caller %0d %0d %s", g.sub_program[0].size(), g.sub_program[1].size(),
             g.sub_program[1][2].nm);
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
    assert_eq!(lines, ["callee 3 sub_3", "caller 0 3 sub_3"]);
}
