//! §4.5, §9.2.2.2: a level-sensitive `always` block that writes nothing (an
//! immediate assertion, a `$display`) is a process of its own. Nothing makes
//! it run before the continuous assignments it reads have settled, so it
//! must not report the values a carry ripples through on its way.
//!
//! The carry vector `c` is written bit by bit by assignments that read it,
//! which the settle orders as a cycle. The block was evaluated within that
//! cycle, once for every carry step, and its assertion failed on each
//! intermediate sum. Verilator runs the same source with no assertion
//! failure.

use xezim::simulate;

const RIPPLE: &str = r#"
module top;
  logic [7:0] a, b, s;
  logic cin;
  logic [8:0] c;
  assign c[0] = cin;
  for (genvar i = 0; i < 8; i++) begin : rc
    assign c[i+1] = (a[i] & b[i]) | ((a[i] ^ b[i]) & c[i]);
  end
  assign s = a ^ b ^ c[7:0];
  always @(*) assert ({c[8], s} == a + b + cin)
    else $display("star failed at %0t: s=%h c8=%b", $time, s, c[8]);
  always @(*) $display("star saw %h at %0t", s, $time);
  initial begin
    a = 0; b = 0; cin = 0; #1;
    a = 8'h7f; b = 8'h01; #1;
    a = 8'hff; cin = 1; #1;
    a = 8'h55; b = 8'haa; cin = 0; #1;
    b = 8'hab; #1;
    $finish;
  end
endmodule
"#;

#[test]
fn level_always_observer_sees_only_settled_values() {
    let sim = simulate(RIPPLE, 1000).expect("simulate failed");
    let out: Vec<String> = sim.output.iter().map(|o| o.message.trim().to_string()).collect();
    let failed: Vec<&String> = out.iter().filter(|l| l.contains("failed")).collect();
    assert!(failed.is_empty(), "{out:#?}");
    // The `$display` block runs once the settle converges: once per input
    // step, on the settled sum (§4.5 allows any order of the two processes;
    // xezim runs the one that writes nothing last).
    let saw: Vec<&String> = out.iter().filter(|l| l.starts_with("star saw")).collect();
    assert_eq!(
        saw,
        ["star saw 00 at 0", "star saw 80 at 1", "star saw 01 at 2", "star saw ff at 3", "star saw 00 at 4"],
        "{out:#?}"
    );
}
