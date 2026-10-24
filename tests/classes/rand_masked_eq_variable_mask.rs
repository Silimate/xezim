//! §18.5, §18.6.3: `(x & M) == C` with a mask that depends on another rand
//! variable is satisfiable whenever `C` lies inside `M`. riscv-dv's PMP NAPOT
//! constraint pins the low `addr_mode` bits of each address, and the solver
//! guessed the address against it: randomize() failed whenever an entry drew
//! NAPOT with a large `addr_mode`.
#[test]
fn masked_equality_with_variable_mask_solves() {
    let result = xezim::simulate(
        r#"
typedef enum bit [1:0] {OFF, TOR, NA4, NAPOT} am_t;
typedef struct { rand am_t a; rand int addr_mode; rand bit [31:0] addr; } e_t;
class pc;
  rand e_t c[];
  function new(); c = new[8]; endfunction
  constraint m_c { foreach (c[i]) { c[i].addr_mode inside {[0:29]}; } }
  constraint o_c { foreach (c[i]) {
    solve c[i].a before c[i].addr;
    solve c[i].addr_mode before c[i].addr; } }
  constraint tor_c { foreach (c[i]) {
    if (i > 0 && c[i].a == TOR && c[i].addr_mode > 0) { c[i].addr > c[i-1].addr; }
    c[i].addr[31:29] == '0; } }
  constraint napot_c { foreach (c[i]) { if (c[i].a == NAPOT) {
    (c[i].addr & ((1 << c[i].addr_mode) - 1)) == ((1 << c[i].addr_mode) - 1);
    (c[i].addr & (1 << c[i].addr_mode)) == 0; } } }
endclass
module t;
  initial begin
    pc p = new;
    int ok = 0, bad = 0, napot = 0, wide = 0;
    for (int k = 0; k < 10; k++) begin
      if (!p.randomize()) continue;
      ok++;
      foreach (p.c[i]) begin
        bit [31:0] m = (1 << p.c[i].addr_mode) - 1;
        if (p.c[i].addr[31:29] != 0) bad++;
        if (p.c[i].a == NAPOT) begin
          napot++;
          if (p.c[i].addr_mode >= 12) wide++;
          if ((p.c[i].addr & m) != m || p.c[i].addr[p.c[i].addr_mode]) bad++;
        end
        if (i > 0 && p.c[i].a == TOR && p.c[i].addr_mode > 0 && p.c[i].addr <= p.c[i-1].addr)
          bad++;
      end
    end
    $display("ok %0d bad %0d", ok, bad);
    $display("napot %0d wide %0d", napot > 0, wide > 0);
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
    assert_eq!(lines, ["ok 10 bad 0", "napot 1 wide 1"]);
}
