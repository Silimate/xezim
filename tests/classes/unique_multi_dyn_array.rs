//! §18.5.5: `unique {a, b}` over rand dynamic arrays makes every element
//! of every listed array pairwise distinct (riscv-dv's
//! `unique {loop_cnt_reg, loop_limit_reg}`). It is satisfiable, so
//! randomize() succeeds every time.
#[test]
fn unique_over_several_dynamic_arrays() {
    let result = xezim::simulate(
        r#"
typedef enum bit [4:0] {R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12, R13, R14, R15,
  R16, R17, R18, R19, R20, R21, R22, R23, R24, R25, R26, R27, R28, R29, R30, R31} reg_t;
class two_c;
  rand reg_t cnt[];
  rand reg_t lim[];
  rand bit [2:0] n;
  constraint k {
    n inside {[1:2]};
    cnt.size() == n;
    lim.size() == n;
    foreach (cnt[i]) cnt[i] != R0;
    unique {cnt, lim};
  }
endclass
class three_c;
  rand bit [3:0] a[], b[], d[];
  constraint k { a.size() == 3; b.size() == 2; d.size() == 4; unique {a, b, d}; }
endclass
module t;
  initial begin
    two_c o = new();
    three_c p = new();
    int ok = 0, bad = 0;
    repeat (4) if (o.randomize()) begin
      ok++;
      if (o.cnt.size() != o.n || o.lim.size() != o.n) bad++;
      foreach (o.cnt[i]) foreach (o.lim[j]) if (o.cnt[i] == o.lim[j]) bad++;
      foreach (o.cnt[i]) foreach (o.cnt[j]) if (i < j && o.cnt[i] == o.cnt[j]) bad++;
      foreach (o.lim[i]) foreach (o.lim[j]) if (i < j && o.lim[i] == o.lim[j]) bad++;
    end
    $display("two %0d %0d", ok, bad);
    ok = 0;
    bad = 0;
    repeat (4) if (p.randomize()) begin
      bit [3:0] all[$];
      all = {p.a, p.b, p.d};
      ok++;
      foreach (all[i]) foreach (all[j]) if (i < j && all[i] == all[j]) bad++;
    end
    $display("three %0d %0d", ok, bad);
    $finish;
  end
endmodule
"#,
        1_000_000,
    )
    .unwrap();
    let lines: Vec<_> = result
        .output
        .iter()
        .map(|entry| entry.message.as_str())
        .collect();
    assert_eq!(lines, ["two 4 0", "three 4 0"]);
}
