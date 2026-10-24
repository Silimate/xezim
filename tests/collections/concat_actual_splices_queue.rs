//! §10.10: `{q}` passed to an unpacked-array formal is an unpacked-array
//! concatenation, so a queue operand contributes all of its elements
//! (riscv-dv `get_rand_instr(.include_instr({jal}))`).
#[test]
fn concat_actual_splices_queue_operands() {
    let result = xezim::simulate(
        r#"
typedef enum {LUI, AUIPC, JAL, JALR, C_J, C_JAL} name_t;
class base_c;
  static function void pick(name_t include_instr[$] = {}, name_t exclude_instr[$] = {});
    string s;
    s = $sformatf("%0d", include_instr.size());
    foreach (include_instr[i]) s = {s, " ", include_instr[i].name()};
    $display("%s", s);
  endfunction
endclass
class s_c;
  name_t jal[$];
  function void run();
    name_t lq[$];
    jal = {JAL, C_J, C_JAL};
    lq = jal;
    base_c::pick(.include_instr({jal}));
    base_c::pick(.include_instr({lq}));
    base_c::pick({jal, JALR});
    base_c::pick(.include_instr({AUIPC, JALR}));
  endfunction
endclass
module t;
  initial begin
    s_c s = new();
    s.run();
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
            "3 JAL C_J C_JAL",
            "3 JAL C_J C_JAL",
            "4 JAL C_J C_JAL JALR",
            "2 AUIPC JALR"
        ]
    );
}
