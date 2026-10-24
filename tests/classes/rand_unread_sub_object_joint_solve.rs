//! §18.5.9: a rand sub-object that no constraint of the class reads shares
//! no constraint with it, so it must not keep the class's own constraints
//! out of the joint solve. riscv-dv's `riscv_instr_gen_config` holds `rand
//! riscv_pmp_cfg pmp_cfg` (a rand dynamic array of structs inside); with
//! `+pmp_randomize=1` its `sum()` constraint went to the per-variable trials
//! and randomize() failed.
#[test]
fn unread_sub_object_keeps_parent_in_joint_solve() {
    let result = xezim::simulate(
        r#"
typedef struct { rand bit [1:0] a; rand bit [7:0] addr; } e_t;
class sub_c;
  rand e_t r[];
  function new(); r = new[4]; endfunction
  constraint s_c { foreach (r[i]) { r[i].addr > 8'd10; } }
endclass
class cfg_c;
  rand sub_c pmp;
  rand int main_cnt;
  rand int sub_cnt[];
  int instr_cnt = 10000;
  constraint default_c {
    sub_cnt.size() == 5;
    main_cnt inside {[10 : instr_cnt]};
    foreach (sub_cnt[i]) { sub_cnt[i] inside {[10 : instr_cnt]}; }
  }
  constraint sum_c { main_cnt + sub_cnt.sum() == instr_cnt; }
  function new(); pmp = new(); endfunction
endclass
module t;
  initial begin
    cfg_c c = new();
    int ok = 0, good = 0;
    for (int k = 0; k < 5; k++) begin
      if (!c.randomize()) continue;
      ok++;
      if (c.main_cnt + c.sub_cnt.sum() == 10000 && c.sub_cnt.size() == 5
          && c.pmp.r[0].addr > 10 && c.pmp.r[3].addr > 10)
        good++;
    end
    $display("ok %0d good %0d", ok, good);
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
    assert_eq!(lines, ["ok 5 good 5"]);
}
