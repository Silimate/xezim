//! The results file's `assertion_instances`: one entry per assertion
//! statement and instance, named and with vacuous successes counted apart.
//! The summed `assertions` entries had no name, added up every instance of a
//! module, and counted `a |-> b` as passing while `a` was false.
//!
//! - IEEE 1800-2017 §16.14.3: a cover statement's results are its attempts,
//!   its successes and its vacuous successes (matches, for a sequence).
//! - §16.14.8: an implication is nonvacuous only when its antecedent matches
//!   and the consequent evaluation it starts is nonvacuous; `not p` is
//!   nonvacuous iff `p` is; `p and q`, `p or q` iff either operand is.
//!   `if (c) p else q` is lowered to implications and gets the same rule.
//! - §16.12: a `disable iff` that holds cancels the attempts in flight and
//!   the one the tick would start.
//!
//! Every count below is the reference simulator's (attempts, passes,
//! vacuous passes, failures and disabled attempts per directive; hits per
//! cover), and so is every name: a label, `<kind>__<n>` for an unlabeled
//! concurrent statement and `immed__<line>` / `i_cover__<line>` for an
//! unlabeled immediate one.
use std::path::PathBuf;
use std::process::Command;

/// Run `files` (name, text) in a fresh directory and return the results
/// file and the run's output.
fn run(tag: &str, files: &[(&str, &str)]) -> (String, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("assertion_instances_{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths: Vec<PathBuf> = files
        .iter()
        .map(|(name, text)| {
            let p = dir.join(name);
            std::fs::write(&p, text).unwrap();
            p
        })
        .collect();
    let db = dir.join("cov.json");
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .env("XEZIM_COV_DB", &db)
        .current_dir(&dir)
        .args(["--no-cache"])
        .args(&paths)
        .output()
        .expect("run xezim");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (std::fs::read_to_string(&db).expect("results file"), text)
}

/// The `assertion_instances` entry of `name` in `instance`.
fn entry<'a>(json: &'a str, instance: &str, name: &str) -> &'a str {
    let head = format!("{{\"instance\": \"{instance}\", \"name\": \"{name}\", ");
    let mut hits = json.lines().filter(|l| l.trim_start().starts_with(&head));
    let line = hits
        .next()
        .unwrap_or_else(|| panic!("no entry for {instance} {name}:\n{json}"));
    assert!(
        hits.next().is_none(),
        "two entries for {instance} {name}:\n{json}"
    );
    line
}

fn field(line: &str, key: &str) -> u64 {
    let at = line
        .find(&format!("\"{key}\": "))
        .unwrap_or_else(|| panic!("no {key} in {line}"));
    line[at + key.len() + 4..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

/// `[attempts, pass, vacuous, fail, disabled]` of an assert or assume.
fn counts(json: &str, instance: &str, name: &str) -> [u64; 5] {
    let e = entry(json, instance, name);
    ["attempts", "pass", "vacuous", "fail", "disabled"].map(|k| field(e, k))
}

fn matches(json: &str, instance: &str, name: &str) -> u64 {
    field(entry(json, instance, name), "matches")
}

const PER_INSTANCE: &str = r#"module sub(input logic clk, input logic rst, input logic a, input logic b);
  a_impl : assert property (@(posedge clk) a |-> b);
  assert property (@(posedge clk) a |=> b);
  c_seq : cover property (@(posedge clk) a ##1 b);
  cover property (@(posedge clk) a |-> b);
  always @(posedge clk) begin
    if (a) a_imm : assert (b);
    assert (!rst || !a);
  end
endmodule

module top;
  logic clk = 0, rst = 1, a = 0, b = 0, c = 0, never = 0;
  integer cyc = 0;
  always #5 clk = ~clk;
  always @(posedge clk) cyc <= cyc + 1;

  sub u_active (.clk(clk), .rst(rst), .a(a), .b(b));
  sub u_idle   (.clk(clk), .rst(rst), .a(never), .b(b));

  a_vac     : assert property (@(posedge clk) never |-> b);
  a_vac_nov : assert property (@(posedge clk) never |=> b);
  a_pf      : assert property (@(posedge clk) a |-> b);
  a_dis     : assert property (@(posedge clk) disable iff (rst) a |-> b);
  a_if      : assert property (@(posedge clk) if (c) b);
  a_ifelse  : assert property (@(posedge clk) if (c) b else !b);
  a_nest    : assert property (@(posedge clk) a |-> (c |-> b));
  a_not     : assert property (@(posedge clk) not (a ##1 c));
  a_bool    : assert property (@(posedge clk) !never);
  a_seqp    : assert property (@(posedge clk) a ##1 b);
  a_and     : assert property (@(posedge clk) (never |-> b) and (a |-> b));
  a_or      : assert property (@(posedge clk) (never |-> b) or (never |-> c));
  m_vac     : assume property (@(posedge clk) never |-> b);
  c_impl    : cover property (@(posedge clk) a |-> b);
  c_vac     : cover property (@(posedge clk) never |-> b);
  c_seq     : cover property (@(posedge clk) a ##1 b);
  c_cs      : cover sequence (@(posedge clk) a ##[0:2] b);
  always @(posedge clk) begin
    i_cov : cover (a && b);
    i_ast : assert (cyc != 7);
  end

  // Sampled at the 10 rising edges (5, 15, ..., 95), in order:
  //   rst 1 1 0 0 0 0 0 0 0 0
  //   a   0 1 1 0 1 0 0 1 0 0
  //   b   0 1 0 1 1 1 0 1 0 0
  //   c   0 0 0 0 1 0 1 0 0 0
  initial begin
    @(negedge clk) a = 1; b = 1;
    @(negedge clk) rst = 0; b = 0;
    @(negedge clk) a = 0; b = 1;
    @(negedge clk) a = 1; c = 1;
    @(negedge clk) a = 0; c = 0;
    @(negedge clk) b = 0; c = 1;
    @(negedge clk) a = 1; b = 1; c = 0;
    @(negedge clk) a = 0; b = 0;
    @(negedge clk);
    @(negedge clk) $finish;
  end
endmodule
"#;

/// Each instance of `sub` has its own entries, and a false antecedent is a
/// vacuous success, not a pass.
#[test]
fn per_instance_entries_count_vacuous_successes_apart() {
    let (json, _) = run("per_instance", &[("a.sv", PER_INSTANCE)]);
    for (inst, name, want) in [
        ("top", "a_vac", [10, 0, 10, 0, 0]),
        ("top", "a_vac_nov", [10, 0, 10, 0, 0]),
        ("top", "a_pf", [10, 3, 6, 1, 0]),
        ("top", "a_dis", [10, 2, 5, 1, 2]),
        ("top", "a_if", [10, 1, 8, 1, 0]),
        ("top", "a_ifelse", [10, 5, 0, 5, 0]),
        ("top", "a_nest", [10, 1, 9, 0, 0]),
        ("top", "a_not", [10, 10, 0, 0, 0]),
        ("top", "a_bool", [10, 10, 0, 0, 0]),
        ("top", "a_seqp", [10, 2, 0, 8, 0]),
        ("top", "a_and", [10, 3, 6, 1, 0]),
        ("top", "a_or", [10, 0, 10, 0, 0]),
        ("top", "m_vac", [10, 0, 10, 0, 0]),
        ("top", "i_ast", [10, 9, 0, 1, 0]),
        ("top.u_active", "a_impl", [10, 3, 6, 1, 0]),
        ("top.u_active", "assert__0", [10, 2, 6, 2, 0]),
        ("top.u_active", "a_imm", [4, 3, 0, 1, 0]),
        ("top.u_active", "immed__8", [10, 9, 0, 1, 0]),
        ("top.u_idle", "a_impl", [10, 0, 10, 0, 0]),
        ("top.u_idle", "assert__0", [10, 0, 10, 0, 0]),
        ("top.u_idle", "immed__8", [10, 10, 0, 0, 0]),
    ] {
        assert_eq!(counts(&json, inst, name), want, "{inst} {name}:\n{json}");
    }
    for (inst, name, want) in [
        ("top", "c_impl", 3),
        ("top", "c_vac", 0),
        ("top", "c_seq", 2),
        ("top", "c_cs", 7),
        ("top", "i_cov", 3),
        ("top.u_active", "c_seq", 2),
        ("top.u_active", "cover__0", 3),
        ("top.u_idle", "c_seq", 0),
        ("top.u_idle", "cover__0", 0),
    ] {
        assert_eq!(matches(&json, inst, name), want, "{inst} {name}:\n{json}");
    }
    // `u_idle` never takes `a_imm`'s branch: no attempt, no entry.
    assert!(
        !json.contains("\"instance\": \"top.u_idle\", \"name\": \"a_imm\""),
        "{json}"
    );
    // The summed entries keep counting a vacuous success as a pass.
    assert!(
        json.contains(
            "\"line\": 21, \"span_start\": 649, \"kind\": \"assert\", \"pass\": 10, \"fail\": 0}"
        ),
        "{json}"
    );
}

const NESTED: &str = r#"module top;
  logic clk = 0, rst = 0, a = 0, b = 0, c = 0, never = 0;
  always #5 clk = ~clk;

  a_dis2  : assert property (@(posedge clk) disable iff (rst) a |-> ##2 b);
  a_nest  : assert property (@(posedge clk) a |-> (c |-> b)) $display("%0t PASS a_nest", $time); else $display("%0t FAIL a_nest", $time);
  a_imp   : assert property (@(posedge clk) a |-> b) $display("%0t PASS a_imp", $time); else $display("%0t FAIL a_imp", $time);
  a_notv  : assert property (@(posedge clk) not (never |-> b));
  a_notr  : assert property (@(posedge clk) not (a |-> b));
  a_orp   : assert property (@(posedge clk) (never |-> b) or (a ##2 b));
  a_nest2 : assert property (@(posedge clk) a |=> (c |-> ##1 b));
  c_nest  : cover property (@(posedge clk) a |-> (c |-> b));
  assert property (@(posedge clk) a |-> b);
  cover property (@(posedge clk) a);
  assume property (@(posedge clk) never |-> b);
  assert property (@(posedge clk) never |-> b);
  a_strong : assert property (@(posedge clk) a |-> strong(##[1:$] never));
  a_weak   : assert property (@(posedge clk) a |-> ##[1:$] never);
  always @(posedge clk) begin
    p_lab : assert property (a |-> b);
    assert property (a |-> b);
    cover (a);
    assert final (a || !a);
  end
  always @(posedge clk) begin : nb
    assert (a || !a);
    lab2 : assert (a || !a);
  end

  // Sampled at the 11 rising edges (5, 15, ..., 105), in order:
  //   rst 0 0 0 0 1 0 0 0 0 0 0
  //   a   0 1 0 1 1 0 1 0 0 1 0
  //   b   0 0 0 1 0 1 1 1 0 1 0
  //   c   0 0 0 1 0 0 1 0 0 0 0
  initial begin
    @(negedge clk) a = 1; b = 0; c = 0;
    @(negedge clk) a = 0; b = 0;
    @(negedge clk) a = 1; b = 1; c = 1;
    @(negedge clk) a = 1; b = 0; c = 0; rst = 1;
    @(negedge clk) a = 0; b = 1; rst = 0;
    @(negedge clk) a = 1; b = 1; c = 1;
    @(negedge clk) a = 0; b = 1; c = 0;
    @(negedge clk) a = 0; b = 0;
    @(negedge clk) a = 1; b = 1; c = 0;
    @(negedge clk) a = 0; b = 0;
    @(negedge clk) $finish;
  end
endmodule
"#;

/// Nested implications, `not`, `or`, an in-flight `disable iff`, strong and
/// weak obligations left at the end, procedural and unlabeled statements.
/// A vacuous success of a nested implication runs no pass action.
#[test]
fn nested_vacuity_disable_and_generated_names() {
    let (json, out) = run("nested", &[("b.sv", NESTED)]);
    for (name, want) in [
        // e2's attempt passes at e4; e4's is in flight and e5's starts when
        // `rst` disables them; e10's is still in flight at the end.
        ("a_dis2", [11, 1, 6, 1, 2]),
        ("a_nest", [11, 2, 9, 0, 0]),
        ("a_imp", [11, 3, 6, 2, 0]),
        // `not` of a vacuous success fails.
        ("a_notv", [11, 0, 0, 11, 0]),
        ("a_notr", [11, 2, 0, 9, 0]),
        // A sequence operand is never vacuous, so the `or` is not either.
        ("a_orp", [11, 11, 0, 0, 0]),
        ("a_nest2", [11, 0, 11, 0, 0]),
        ("a_strong", [11, 0, 6, 5, 0]),
        ("a_weak", [11, 0, 6, 0, 0]),
        ("assert__0", [11, 3, 6, 2, 0]),
        ("assert__1", [11, 0, 11, 0, 0]),
        ("assume__0", [11, 0, 11, 0, 0]),
        ("p_lab", [11, 3, 6, 2, 0]),
        ("assert__2", [11, 3, 6, 2, 0]),
        ("immed__23", [11, 11, 0, 0, 0]),
        ("nb.immed__26", [11, 11, 0, 0, 0]),
        ("nb.lab2", [11, 11, 0, 0, 0]),
    ] {
        assert_eq!(counts(&json, "top", name), want, "{name}:\n{json}");
    }
    for (name, want) in [("c_nest", 2), ("cover__0", 5), ("i_cover__22", 5)] {
        assert_eq!(matches(&json, "top", name), want, "{name}:\n{json}");
    }
    let actions: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("PASS") || l.contains("FAIL"))
        .collect();
    assert_eq!(
        actions,
        [
            "15 FAIL a_imp",
            "35 PASS a_nest",
            "35 PASS a_imp",
            "45 FAIL a_imp",
            "65 PASS a_nest",
            "65 PASS a_imp",
            "95 PASS a_imp"
        ],
        "{out}"
    );
}

const PKG: &str = r#"package chk_pkg;
  function automatic bit in_range(input int v);
    in_range = (v < 100);
    a_pkg : assert (v >= 0);
    return in_range;
  endfunction
endpackage
"#;

const LEAF: &str = r#"module leaf;
  int count = 0;
  function automatic void bump(input int v);
    count = count + 1;
    a_bump : assert (v != 3);
  endfunction
endmodule
"#;

const TOP: &str = r#"module gen_leaf(input logic clk, input logic [1:0] d);
  a_leaf : assert property (@(posedge clk) d == d);
  always @(posedge clk) begin : blk
    i_leaf : assert (d == d);
  end
endmodule

module top;
  import chk_pkg::*;
  logic clk = 0;
  logic [1:0] v = 0;
  always #5 clk = ~clk;
  leaf u0 ();
  for (genvar i = 0; i < 2; i++) begin : g
    a_gen : assert property (@(posedge clk) v == v);
    gen_leaf u (.clk(clk), .d(v));
  end
  initial begin
    for (int i = 0; i < 4; i++) begin
      u0.bump(i);
      void'(in_range(i));
    end
    #12 $finish;
  end
endmodule
"#;

/// An assertion in a subroutine belongs to the scope that declares the
/// subroutine, and its file and line are that scope's, in both lists: they
/// were the calling process's file with the statement's offset. Generate
/// blocks are part of the name, not the instance.
#[test]
fn entries_name_the_declaring_scope_and_its_file() {
    let (json, _) = run(
        "scopes",
        &[("pkg.sv", PKG), ("leaf.sv", LEAF), ("top.sv", TOP)],
    );
    let bump = entry(&json, "top.u0", "bump.a_bump");
    assert!(bump.contains("leaf.sv\", \"line\": 5, "), "{json}");
    assert_eq!(counts(&json, "top.u0", "bump.a_bump"), [4, 3, 0, 1, 0]);
    let pkg = entry(&json, "chk_pkg", "in_range.a_pkg");
    assert!(pkg.contains("pkg.sv\", \"line\": 4, "), "{json}");
    assert_eq!(counts(&json, "chk_pkg", "in_range.a_pkg"), [4, 4, 0, 0, 0]);
    for (inst, name) in [
        ("top", "g[0].a_gen"),
        ("top", "g[1].a_gen"),
        ("top.g[0].u", "a_leaf"),
        ("top.g[1].u", "blk.i_leaf"),
    ] {
        assert_eq!(
            counts(&json, inst, name),
            [1, 1, 0, 0, 0],
            "{inst} {name}:\n{json}"
        );
    }
    let summed: Vec<&str> = json
        .lines()
        .skip_while(|l| !l.contains("\"assertions\": ["))
        .take_while(|l| !l.contains("\"assertion_instances\""))
        .collect();
    assert!(
        summed
            .iter()
            .any(|l| l.contains("leaf.sv\", \"line\": 5, ")),
        "{json}"
    );
    assert!(
        summed.iter().any(|l| l.contains("pkg.sv\", \"line\": 4, ")),
        "{json}"
    );
}

const SETTLE: &str = r#"module leaf;
  function automatic void bump(input int v);
    a_bump : assert (v != 3);
  endfunction
endmodule

module top;
  logic clk = 0, a = 0, b = 0, c = 0, never = 0, go = 0;
  always #5 clk = ~clk;
  leaf u0 ();
  leaf u1 ();
  a_or_impl   : assert property (@(posedge clk) (never |-> b) or (a |-> ##2 b));
  a_or_strong : assert property (@(posedge clk) (never |-> b) or (a |-> strong(##[1:$] never)));
  a_or_rev    : assert property (@(posedge clk) (a |-> ##2 b) or (never |-> b));
  a_and_seq   : assert property (@(posedge clk) (never |-> b) and (a ##2 b));
  a_notimpl   : assert property (@(posedge clk) not ((1 ##[0:1] 1) |-> not (c |-> 0)));
  always @(posedge clk) begin
    if (go) assert property (a |-> b);
    assert property (a |-> b);
  end
  initial begin
    for (int i = 0; i < 4; i++) begin
      u0.bump(i);
      u1.bump(i + 1);
    end
  end
  // a: 0 1 0 1 1 0 0 ; b: 0 0 1 1 0 1 0 ; c: 0 1 0 0 0 0 0
  initial begin
    @(negedge clk) begin a = 1; c = 1; end
    @(negedge clk) begin a = 0; b = 1; c = 0; end
    @(negedge clk) begin a = 1; b = 1; end
    @(negedge clk) begin a = 1; b = 0; end
    @(negedge clk) begin a = 0; b = 1; end
    @(negedge clk) begin a = 0; b = 0; end
    @(negedge clk) $finish;
  end
endmodule
"#;

/// `p or q` is settled as soon as an operand holds, and is nonvacuous when
/// the other operand's evaluation already is (an implication whose
/// antecedent matched): it neither waits nor fails at the end for a strong
/// obligation left in the other operand. A failure is never vacuous, so
/// `not` of one is a nonvacuous success. One process calling the same
/// function of two instances counts each instance apart, and unlabeled
/// statements are numbered by declaration, a branch never taken included.
#[test]
fn or_settles_early_and_names_follow_declarations() {
    let (json, _) = run("settle", &[("t.sv", SETTLE)]);
    for (inst, name, want) in [
        ("top", "a_or_impl", [7, 3, 4, 0, 0]),
        ("top", "a_or_strong", [7, 3, 4, 0, 0]),
        ("top", "a_or_rev", [7, 3, 4, 0, 0]),
        ("top", "a_and_seq", [7, 2, 0, 5, 0]),
        ("top", "a_notimpl", [7, 7, 0, 0, 0]),
        ("top", "assert__1", [7, 1, 4, 2, 0]),
        ("top.u0", "bump.a_bump", [4, 3, 0, 1, 0]),
        ("top.u1", "bump.a_bump", [4, 3, 0, 1, 0]),
    ] {
        assert_eq!(counts(&json, inst, name), want, "{inst} {name}:\n{json}");
    }
}

const PROCEDURAL: &str = r#"module pm #(parameter int W = 1) (input logic clk, input logic [W-1:0] d, input logic go);
  always @(posedge clk) begin
    if (go) assert property (d == d);
    assert property (d == d);
  end
  cover property (@(posedge clk) d == d);
endmodule
module top;
  logic clk = 0;
  logic [3:0] v = 0;
  always #5 clk = ~clk;
  pm #(.W(2)) u2 (.clk(clk), .d(v[1:0]), .go(1'b0));
  pm #(.W(4)) u4 (.clk(clk), .d(v), .go(1'b0));
  initial #22 $finish;
endmodule
"#;

/// A concurrent assertion in an `always` block of a module instantiated
/// twice gets an entry per instance: both instances' attempts used to go to
/// the first instance's.
#[test]
fn procedural_assertions_count_per_instance() {
    let (json, _) = run("procedural", &[("t.sv", PROCEDURAL)]);
    for inst in ["top.u2", "top.u4"] {
        assert_eq!(
            counts(&json, inst, "assert__1"),
            [2, 2, 0, 0, 0],
            "{inst}:\n{json}"
        );
        assert_eq!(matches(&json, inst, "cover__0"), 2, "{inst}:\n{json}");
    }
}

const CLASS_A: &str = r#"package pa;
  class ca;
    function void f(int v);
      a_c : assert (v != 1);
      assert (v != 3);
    endfunction
  endclass
endpackage
"#;

const CLASS_B: &str = r#"package pb;
  class cb;
    function void f(int v);
      a_c : assert (v != 2);
      assert (v != 0);
    endfunction
  endclass
endpackage
"#;

const CASES: &str = r#"module top;
  logic clk = 0, a = 0, b = 0;
  logic [1:0] sel = 2'd1;
  always #5 clk = ~clk;
  pa::ca x = new;
  pb::cb y = new;
  always @(posedge clk) begin
    case (sel)
      2'd0: assert property (a |-> b);
      2'd1: assert property (b || !b);
      2'b10, 2'b11: lab_c : assert property (a);
    endcase
    case (sel)
      2'd0: if (a) lab_d : assert property (b);
      default: begin : blk
        assert property (b || !b);
      end : blk
    endcase
    case (sel)
      a ? 2'd0 : 2'd3: assert property (a |-> b);
      default: assert property (a || !a);
    endcase
    casez (sel)
      2'b1?: assert property (b || !b);
    endcase
    case (1'b1)
      sel ==? 2'b11: assert property (a |-> b);
      default: assert property (b || !b);
    endcase
  end
  assert property (@(posedge clk) a || !a);
  initial begin
    for (int i = 0; i < 4; i++) begin
      x.f(i);
      y.f(i);
    end
    #22 sel = 2'd2;
    #20 $finish;
  end
endmodule
"#;

/// The methods of two classes declared in different files, with their
/// statements at the same offsets and their objects created in the same
/// instance, keep an entry each, under their package and with their own
/// file. A case item is not a label: `2'd1 : assert property ...` is the
/// second unlabeled statement, `assert__1`, whether or not the first item
/// ever runs, and `lab_d`, labeled after an item and an `if`, takes no
/// number though it never runs. Neither the colon of a conditional
/// operator in an item, nor the `?` digit of a `casez` item, nor the `?` of
/// a wildcard equality ends the item. (The reference simulator also lists
/// the unreached `assert__0`, `assert__3`, `assert__6` and `lab_d` with
/// zero counts; xezim lists a procedural statement once reached.)
#[test]
fn class_methods_and_case_items_keep_their_own_entries() {
    let (json, _) = run(
        "classes",
        &[("ca.sv", CLASS_A), ("cb.sv", CLASS_B), ("top.sv", CASES)],
    );
    for (inst, name, want) in [
        ("pa", "ca.f.a_c", [4, 3, 0, 1, 0]),
        ("pa", "ca.f.immed__5", [4, 3, 0, 1, 0]),
        ("pb", "cb.f.a_c", [4, 3, 0, 1, 0]),
        ("pb", "cb.f.immed__5", [4, 3, 0, 1, 0]),
        ("top", "assert__1", [2, 2, 0, 0, 0]),
        ("top", "lab_c", [2, 0, 0, 2, 0]),
        ("top", "blk.assert__2", [4, 4, 0, 0, 0]),
        ("top", "assert__4", [4, 4, 0, 0, 0]),
        ("top", "assert__5", [2, 2, 0, 0, 0]),
        ("top", "assert__7", [4, 4, 0, 0, 0]),
        ("top", "assert__8", [4, 4, 0, 0, 0]),
    ] {
        assert_eq!(counts(&json, inst, name), want, "{inst} {name}:\n{json}");
    }
    assert!(
        entry(&json, "pb", "cb.f.a_c").contains("cb.sv\", \"line\": 4, "),
        "{json}"
    );
    for name in ["assert__0", "assert__3", "assert__6", "lab_d"] {
        assert!(
            !json.contains(&format!("\"instance\": \"top\", \"name\": \"{name}\"")),
            "{json}"
        );
    }
}

const SETTLED: &str = r#"module unit(output logic o);
  for (genvar k = 0; k < 2; k++) begin : gb
    logic [3:0] datum = k;
  end
  assign o = gb[1].datum == 1;
endmodule

module top;
  logic clk = 0, o;
  int hits = 0;
  unit u (.o(o));
  always #5 clk = ~clk;
  for (genvar i = 0; i < 3; i++) begin : g
    a_gen : assert property (@(posedge clk) i < 3) hits += i + 1;
  end
  a_top : assert property (@(posedge clk) o);
  initial begin
    #22 $display("hits=%0d", hits);
    $finish;
  end
endmodule
"#;

/// §16.5, §16.14: declarative concurrent assertions register before time
/// 0's processes run, after the time-0 settle has evaluated `u`'s
/// continuous assignment. They still belong to `top`, one per generate
/// block, each running its action at both edges (5 and 15). The settle's
/// last block scope named them after `top.u` instead, so the three generate
/// blocks shared one site and `hits` was 2.
#[test]
fn sites_registered_after_the_time_0_settle_keep_their_instance() {
    let (json, out) = run("settled", &[("s.sv", SETTLED)]);
    for name in ["g[0].a_gen", "g[1].a_gen", "g[2].a_gen", "a_top"] {
        assert_eq!(counts(&json, "top", name), [2, 2, 0, 0, 0], "{name}:\n{json}");
    }
    assert!(!json.contains("\"instance\": \"top.u\""), "{json}");
    assert!(out.contains("hits=12"), "{out}");
}
