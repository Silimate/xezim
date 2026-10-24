//! §8.9 / §10.10: inside a class body, `C::aq[k]` (an element of a static
//! associative array of queues) is a whole queue, both as an operand of an
//! unpacked-array concatenation and as the source of a copy.
#[test]
fn class_scoped_assoc_queue_element_inside_class_body() {
    let result = xezim::simulate(
        r#"
typedef enum { ADD, SUB, SLL, JAL, BEQ } name_t;
typedef enum { ARITH, SHIFT, JUMP } cat_t;
class instr_c;
  static name_t cat[cat_t][$];
  static name_t basic[$];
  static function void build();
    cat[ARITH].push_back(ADD);
    cat[ARITH].push_back(SUB);
    cat[SHIFT].push_back(SLL);
    cat[JUMP].push_back(JAL);
    cat[JUMP].push_back(BEQ);
    basic = {cat[ARITH], cat[SHIFT]};
  endfunction
endclass
class stream_c;
  name_t allowed[$];
  name_t jumps[$];
  function void setup();
    allowed = instr_c::basic;
    allowed = {allowed, instr_c::cat[JUMP]};
    jumps = instr_c::cat[JUMP];
  endfunction
endclass
module t;
  initial begin
    stream_c s = new();
    instr_c::build();
    s.setup();
    $display("concat %0d %s %s", s.allowed.size(), s.allowed[3].name(), s.allowed[4].name());
    $display("copy %0d %s", s.jumps.size(), s.jumps[1].name());
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
    assert_eq!(lines, ["concat 5 JAL BEQ", "copy 2 BEQ"]);
}
