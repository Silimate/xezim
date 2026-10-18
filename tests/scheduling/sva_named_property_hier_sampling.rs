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
//! ticked. A reference into a generate block or an instance-array element
//! (`stg[1].r`) stays a member access over a select even in a process
//! body, so it was not sampled inline either (`a13`), and `$past` of it had
//! no history (`a16`).
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
  for (genvar k = 0; k < 2; k++) begin : gb
    logic [1:0] x = 0;
    always @(posedge clk) x <= (x == 0 && go) ? 2'd1 : 2'd0;
  end
endmodule
module chk(input logic clk, input logic go);    // a named property below the top
  int sn = 0, sf = 0, gn = 0, gf = 0, pn = 0, pf = 0;
  core u(.clk(clk), .go(go));
  for (genvar j = 0; j < 2; j++) begin : cg
    logic [1:0] y = 0;
    always @(posedge clk) y <= (y == 0 && go) ? 2'd1 : 2'd0;
  end
  property p_sub;
    @(posedge clk) (u.st == 0 && go) |=> u.st == 1;
  endproperty
  property p_cg;                                // into this instance's generate block
    @(posedge clk) (cg[1].y == 0 && go) |=> cg[1].y == 1;
  endproperty
  property p_cgpast;
    @(posedge clk) cg[1].y == 1 |-> $past(cg[1].y) == 0 && $past(go);
  endproperty
  a_sub: assert property (p_sub) sn++; else sf++;
  a_cg: assert property (p_cg) gn++; else gf++;
  a_cgpast: assert property (p_cgpast) pn++; else pf++;
endmodule
module t;
  logic clk = 0, go = 0;
  int n [17], f [17];
  core u(.clk(clk), .go(go));
  core ua [1:0] (.clk(clk), .go(go));
  chk c(.clk(clk), .go(go));
  always #5 clk = ~clk;
  for (genvar g = 0; g < 2; g++) begin : stg
    logic [1:0] r = 0;
    always @(posedge clk) r <= (r == 0 && go) ? 2'd1 : 2'd0;
  end
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
  property p_gen;                               // into a generate block
    @(posedge clk) (stg[1].r == 0 && go) |=> stg[1].r == 1;
  endproperty
  property p_subgen;
    @(posedge clk) (u.gb[1].x == 0 && go) |=> u.gb[1].x == 1;
  endproperty
  property p_arr;                               // into an instance-array element
    @(posedge clk) (ua[1].st == 0 && go) |=> ua[1].st == 1;
  endproperty
  property p_genpast;                           // $past of a generate-block reference
    @(posedge clk) stg[1].r == 1 |-> $past(stg[1].r) == 0 && $past(go);
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
  a12: assert property (p_gen) n[12]++; else f[12]++;
  a13: assert property (@(posedge clk) (stg[1].r == 0 && go) |=> stg[1].r == 1) n[13]++; else f[13]++;
  a14: assert property (p_subgen) n[14]++; else f[14]++;
  a15: assert property (p_arr) n[15]++; else f[15]++;
  a16: assert property (p_genpast) n[16]++; else f[16]++;
  initial begin
    @(negedge clk) go = 1;
    @(negedge clk) go = 0;
    repeat (3) @(negedge clk);
    for (int i = 0; i < 17; i++)
      $display("a%0d %s pass=%0d fail=%0d", i, n[i] == 1 && f[i] == 0 ? "PASS" : "FAIL", n[i], f[i]);
    $display("a_sub %s pass=%0d fail=%0d", c.sn == 1 && c.sf == 0 ? "PASS" : "FAIL", c.sn, c.sf);
    $display("a_cg %s pass=%0d fail=%0d", c.gn == 1 && c.gf == 0 ? "PASS" : "FAIL", c.gn, c.gf);
    $display("a_cgpast %s pass=%0d fail=%0d", c.pn == 1 && c.pf == 0 ? "PASS" : "FAIL", c.pn, c.pf);
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
    let names = (0..17)
        .map(|i| format!("a{i}"))
        .chain(["a_sub", "a_cg", "a_cgpast"].map(String::from));
    for name in names {
        assert!(
            text.contains(&format!("{name} PASS pass=1 fail=0")),
            "{name}: expected one success and no failure:\n{text}"
        );
    }
}

/// §16.5.1: a member of an unpacked struct array element picked by a
/// variable (`a[i].f`) is read with `i`'s sampled value at every tick. Only
/// a select fixed at elaboration (a generate block's or an instance array's)
/// is folded into a name; folding this one froze the element `i` named when
/// the assertion registered, so every other tick checked the wrong element.
#[test]
fn variable_selects_in_assertions_follow_the_index() {
    let text = run(
        "index",
        r#"
module t;
  typedef struct { logic [1:0] f; } e_t;
  logic clk = 0;
  e_t a [2];
  int i = 0, n = 0, f = 0, m = 0, k = 0;
  initial begin a[0].f = 1; a[1].f = 2; end
  always #5 clk = ~clk;
  always @(posedge clk) i <= 1 - i;
  property p_idx;
    @(posedge clk) a[i].f == i + 1;
  endproperty
  a_named: assert property (p_idx) n++; else f++;
  a_inline: assert property (@(posedge clk) a[i].f == i + 1) m++; else k++;
  initial begin
    #60;
    $display("named pass=%0d fail=%0d inline pass=%0d fail=%0d", n, f, m, k);
    $finish;
  end
endmodule
"#,
    );
    assert!(
        text.contains("named pass=6 fail=0 inline pass=6 fail=0"),
        "{text}"
    );
}
