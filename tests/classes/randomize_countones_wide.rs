//! `$countones` of a vector wider than 64 bits (§20.9).
//!
//! `std::randomize(data_mask) with {$countones(data_mask) inside {[1:2]};}`
//! in `ibex_icache_ram_if::gen_data_err` randomizes a 78-bit mask. The count
//! of a whole variable was a solver variable only at 64 bits or fewer, so
//! the call returned 0.

use xezim::simulate;

#[test]
fn countones_wide_inside_one_or_two() {
    let src = r#"
module t;
  bit [77:0] m;
  int i, r, n, bad;
  initial begin
    for (i = 0; i < 8; i++) begin
      r = std::randomize(m) with { $countones(m) inside {[1:2]}; };
      n = $countones(m);
      if (r != 1 || !(n inside {[1:2]})) bad++;
    end
    r = std::randomize(m) with { $countones(m) == 80; };
    $display("RESULT bad=%0d unsat=%0d", bad, r);
  end
endmodule
"#;
    let out: Vec<String> = simulate(src, 1000)
        .expect("simulate failed")
        .output
        .iter()
        .map(|o| o.message.clone())
        .filter(|m| m.contains("RESULT"))
        .collect();
    assert_eq!(out, vec!["RESULT bad=0 unsat=0".to_string()]);
}
