//! §16.5.1 — the operands of a named property or sequence are sampled in the
//! Preponed region, like those of an assertion written inline, and that holds
//! for a hierarchical reference (`u.st`) too.
//!
//! A hierarchical reference in a top-level or package `property` /
//! `sequence` declaration kept the parser's member-access shape, which the
//! elaborator folds into a hierarchical name only in process bodies and in
//! an inlined instance's declarations. The assertion did not sample it, so
//! it read the value the clock edge's nonblocking assignment had just
//! written: an antecedent on a state changing at that edge never matched
//! (`p_start` passed only vacuously) and a consequent failed (`p_cons`). A
//! hierarchical clock (`p_clk`) named no signal, so that property never
//! ticked.
//!
//! `u.st` goes 0 -> 1 at the 15 ns edge and back to 0 at 25 ns. Each
//! assertion below has exactly one nonvacuous success and no failure; the
//! reference simulator reports the same counts.

use std::path::PathBuf;
use std::process::Command;

const DESIGN: &str = r#"
package pk;
  property p_pk(x, g, c);                       // formals bound to hierarchical actuals
    @(posedge c) (x == 0 && g) |=> x == 1;
  endproperty
endpackage
module core(input logic clk, input logic go);
  logic [1:0] st = 0;
  always @(posedge clk) st <= (st == 0 && go) ? 2'd1 : 2'd0;
endmodule
module chk(input logic clk, input logic go);    // a named property below the top
  int sn = 0, sf = 0;
  core u(.clk(clk), .go(go));
  property p_sub;
    @(posedge clk) (u.st == 0 && go) |=> u.st == 1;
  endproperty
  a_sub: assert property (p_sub) sn++; else sf++;
endmodule
module t;
  logic clk = 0, go = 0;
  int n [12], f [12];
  core u(.clk(clk), .go(go));
  chk c(.clk(clk), .go(go));
  always #5 clk = ~clk;
  property p_start;                             // antecedent on u.st
    @(posedge clk) (u.st == 0 && go) |=> u.st == 1;
  endproperty
  property p_cons;                              // consequent on u.st
    @(posedge clk) go |=> u.st == 1;
  endproperty
  property p_bit;
    @(posedge clk) (u.st[0] == 0 && go) |=> u.st[0] == 1;
  endproperty
  property p_abs;
    @(posedge clk) (t.u.st == 0 && go) |=> t.u.st == 1;
  endproperty
  sequence s_idle;
    u.st == 0 && go;
  endsequence
  sequence s_x(x);
    x == 0 && go;
  endsequence
  property p_seq;                               // a named sequence in a named property
    @(posedge clk) s_idle |=> u.st == 1;
  endproperty
  property p_seqf;                              // a sequence formal bound to u.st
    @(posedge clk) s_x(u.st) |=> u.st == 1;
  endproperty
  property p_f(x);
    @(posedge clk) (x == 0 && go) |=> x == 1;
  endproperty
  property p_past;
    @(posedge clk) u.st == 1 |-> $past(u.st) == 0 && $past(go);
  endproperty
  property p_rose;
    @(posedge clk) $rose(u.st) && u.st == 1 |-> $past(go);
  endproperty
  property p_clk;                               // a hierarchical clock
    @(posedge u.clk) (u.st == 0 && go) |=> u.st == 1;
  endproperty
  a0: assert property (p_start) n[0]++; else f[0]++;
  a1: assert property (p_cons) n[1]++; else f[1]++;
  a2: assert property (p_bit) n[2]++; else f[2]++;
  a3: assert property (p_abs) n[3]++; else f[3]++;
  a4: assert property (@(posedge clk) s_idle |=> u.st == 1) n[4]++; else f[4]++;
  a5: assert property (p_seq) n[5]++; else f[5]++;
  a6: assert property (p_seqf) n[6]++; else f[6]++;
  a7: assert property (p_f(u.st)) n[7]++; else f[7]++;
  a8: assert property (pk::p_pk(u.st, go, clk)) n[8]++; else f[8]++;
  a9: assert property (p_past) n[9]++; else f[9]++;
  a10: assert property (p_rose) n[10]++; else f[10]++;
  a11: assert property (p_clk) n[11]++; else f[11]++;
  initial begin
    @(negedge clk) go = 1;
    @(negedge clk) go = 0;
    repeat (3) @(negedge clk);
    for (int i = 0; i < 12; i++)
      $display("a%0d %s pass=%0d fail=%0d", i, n[i] == 1 && f[i] == 0 ? "PASS" : "FAIL", n[i], f[i]);
    $display("a_sub %s pass=%0d fail=%0d", c.sn == 1 && c.sf == 0 ? "PASS" : "FAIL", c.sn, c.sf);
    $finish;
  end
endmodule
"#;

fn run(name: &str, src: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sva_named_property_hier_sampling");
    std::fs::create_dir_all(&dir).unwrap();
    let sv = dir.join(format!("{name}.sv"));
    std::fs::write(&sv, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--simulate", "-s", "t", "--no-cache", sv.to_str().unwrap()])
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "run failed:\n{text}");
    text
}

/// §3.12.1: `$unit::g` in a named property still names the compilation-unit
/// variable where the module declares its own `g`. Folding the declaration
/// (`u.v` makes it fold) must keep the qualifier there, as the elaborator
/// does in process bodies.
#[test]
fn unit_qualified_names_in_named_properties_keep_their_scope() {
    let text = run(
        "unit",
        r#"
logic g = 0;
module m;
  logic v = 0;
endmodule
module t;
  logic clk = 0;
  logic g = 1;
  int n = 0, f = 0;
  m u();
  always #5 clk = ~clk;
  property p_unit;
    @(posedge clk) $unit::g == 0 && g == 1 && u.v == 0;
  endproperty
  a: assert property (p_unit) n++; else f++;
  initial begin
    #42;
    $display("unit pass=%0d fail=%0d", n, f);
    $finish;
  end
endmodule
"#,
    );
    assert!(text.contains("unit pass=4 fail=0"), "{text}");
}

#[test]
fn hierarchical_references_in_named_properties_are_sampled() {
    let text = run("hier", DESIGN);
    let names = (0..12)
        .map(|i| format!("a{i}"))
        .chain(["a_sub".to_string()]);
    for name in names {
        assert!(
            text.contains(&format!("{name} PASS pass=1 fail=0")),
            "{name}: expected one success and no failure:\n{text}"
        );
    }
}
