//! §8.9 / §7.6: `ClassName::q` names a static queue or dynamic array as a
//! whole, so assigning it copies every element into a module variable or a
//! class property, the same as a read from inside the class.
#[test]
fn class_scoped_static_queue_copies_whole() {
    let result = xezim::simulate(
        r#"
typedef enum { ADD, SUB, XOR_I, SLL, SLT, JAL } name_t;
class instr_c;
  static name_t cat[int][$];
  static name_t basic[$];
  static int dyn[];
  static function void build();
    cat[0].push_back(ADD);
    cat[0].push_back(SUB);
    cat[1].push_back(SLL);
    cat[1].push_back(SLT);
    cat[2].push_back(JAL);
    basic = {cat[0], cat[1]};
    dyn = new[3];
    foreach (dyn[i]) dyn[i] = 10 * (i + 1);
  endfunction
endclass
class base_c;
  name_t allowed[$];
endclass
class stream_c extends base_c;
  int d[];
  function void setup();
    allowed = instr_c::basic;
    d = instr_c::dyn;
  endfunction
endclass
module t;
  initial begin
    stream_c s = new();
    name_t q[$];
    int d[];
    instr_c::build();
    q = instr_c::basic;
    d = instr_c::dyn;
    $display("module %0d %s %0d %0d", q.size(), q[3].name(), d.size(), d[2]);
    s.setup();
    $display("method %0d %s %0d %0d", s.allowed.size(), s.allowed[3].name(), s.d.size(), s.d[1]);
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
    assert_eq!(lines, ["module 4 SLT 3 30", "method 4 SLT 3 20"]);
}
