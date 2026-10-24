//! §7.8 / §7.6: `q = aq[k]` where `aq` is an associative array of queues
//! copies the whole queue element, for any key type and for an object's
//! property read inside or outside its methods.
#[test]
fn assoc_of_queue_element_copies_whole() {
    let result = xezim::simulate(
        r#"
typedef enum { ADD, SUB, JAL, BEQ } name_t;
typedef enum { ARITH, JUMP } cat_t;
class h_c;
  name_t cat[cat_t][$];
  function void fill();
    name_t in_obj[$];
    cat[JUMP].push_back(JAL);
    cat[JUMP].push_back(BEQ);
    in_obj = cat[JUMP];
    $display("method %0d %s", in_obj.size(), in_obj[1].name());
  endfunction
endclass
module t;
  int mi[int][$];
  int ms[string][$];
  initial begin
    int j[$];
    name_t k[$];
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
    h.fill();
    k = h.cat[JUMP];
    $display("handle %0d %s", k.size(), k[0].name());
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
        ["int 2 6", "string 3 10", "method 2 BEQ", "handle 2 JAL"]
    );
}
