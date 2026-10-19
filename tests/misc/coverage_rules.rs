//! The other counting rules of code coverage (docs/coverage-guide.md,
//! "Counting rules"), each off by default: `--no-toggle-enums`,
//! `--no-toggle-generate`, `--code-coverage-expressions=logical`,
//! `--code-coverage-granularity=block`, `--no-fsm-reset-transitions` and
//! `--no-toggle-tied`.
use std::path::{Path, PathBuf};
use std::process::Command;

fn xezim() -> String {
    let mut p = std::env::current_exe().expect("current_exe");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("xezim").to_string_lossy().into_owned()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xezim_cov_rules_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run xezim on `src` in `dir` with `args`, and return the results file.
fn run(dir: &Path, src: &str, args: &[&str]) -> String {
    std::fs::write(dir.join("t.sv"), src).unwrap();
    let out = Command::new(xezim())
        .current_dir(dir)
        .env("XEZIM_COV_DB", dir.join("cov.json"))
        .env_remove("XEZIM_CODE_COVERAGE")
        .env_remove("XEZIM_TOGGLE_ENUMS")
        .env_remove("XEZIM_TOGGLE_GENERATE")
        .env_remove("XEZIM_TOGGLE_INTEGERS")
        .env_remove("XEZIM_TOGGLE_INPUT_PORTS")
        .env_remove("XEZIM_CODE_COVERAGE_EXPRESSIONS")
        .env_remove("XEZIM_CODE_COVERAGE_GRANULARITY")
        .env_remove("XEZIM_FSM_RESET_TRANSITIONS")
        .env_remove("XEZIM_TOGGLE_TIED")
        .args(args)
        .arg("t.sv")
        .output()
        .expect("run xezim");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_to_string(dir.join("cov.json")).unwrap()
}

/// The text of scope `name`'s entry in a results file.
fn scope<'a>(json: &'a str, name: &str) -> &'a str {
    let at = json
        .find(&format!("\"scope\": \"{name}\","))
        .unwrap_or_else(|| panic!("no scope {name}: {json}"));
    let rest = &json[at..];
    let end = [
        rest[1..].find("\"scope\": \""),
        rest.find("\"design_units\""),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(rest.len());
    &rest[..end]
}

/// The values of `key` (a string field) in the order they appear.
fn strings<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let pat = format!("\"{key}\": \"");
    text.split(&pat)
        .skip(1)
        .map(|s| &s[..s.find('"').unwrap()])
        .collect()
}

const TOGGLES: &str = "\
typedef enum logic [1:0] {A, B, C} st_t;
module sub #(parameter N = 2) (input logic clk, output logic [1:0] o);
  logic [1:0] r;
  if (N > 1) begin
    logic [1:0] ub;
    struct { logic m; } gs;
    always @(posedge clk) begin ub <= ub + 1; gs.m <= ~gs.m; end
  end
  if (N > 1) begin : lb
    logic [1:0] lab;
    always @(posedge clk) lab <= lab + 1;
  end
  for (genvar i = 0; i < 2; i++) begin : fl
    logic f;
    always @(posedge clk) f <= ~f;
  end
  always @(posedge clk) r <= r + 1;
  assign o = r;
endmodule
module leaf(input logic clk);
  for (genvar i = 0; i < 2; i++) begin
    logic g2;
    always @(posedge clk) g2 <= ~g2;
  end
  logic keep;
  always @(posedge clk) keep <= ~keep;
endmodule
module tb;
  logic clk = 0;
  st_t st;
  struct { logic m; logic [1:0] k; } us;
  struct { logic m; } genblk_status;
  wire [1:0] o;
  sub u(.clk, .o);
  leaf w(.clk);
  always #5 clk = ~clk;
  initial begin st = A; us.m = 0; us.k = 0; genblk_status.m = 0; #12 st = C; us.m = 1; us.k = 3; genblk_status.m = 1; #20 $finish; end
endmodule
";

/// `--no-toggle-enums` leaves enum variables out of toggle coverage;
/// `--no-toggle-generate` leaves out the signals declared in a generate
/// block: an unlabeled branch's (`ub`, bare in its instance, and the
/// members of its struct `gs`), a labeled one's (`lb.lab`), a named loop's
/// (`fl[0].f`) and an unnamed loop's (`genblk1[0].g2`, in a module with no
/// other generate block). Module-scope signals, ports and the members of a
/// module-scope unpacked struct stay, `genblk_status` too: only an unnamed
/// loop's own `genblk<n>` (§27.6) names a block. By default both are
/// counted.
#[test]
fn toggle_enums_and_generate_blocks_left_out() {
    let d = scratch("toggles");
    let all = run(&d, TOGGLES, &["--code-coverage=toggle"]);
    assert_eq!(
        strings(scope(&all, "tb"), "signal"),
        [
            "clk",
            "genblk_status",
            "genblk_status.m",
            "o",
            "st",
            "us",
            "us.k",
            "us.m"
        ]
    );
    assert_eq!(
        strings(scope(&all, "tb.u"), "signal"),
        [
            "clk", "fl[0].f", "fl[1].f", "gs.m", "lb.lab", "o", "r", "ub"
        ]
    );
    assert_eq!(
        strings(scope(&all, "tb.w"), "signal"),
        ["clk", "genblk1[0].g2", "genblk1[1].g2", "keep"]
    );
    let enums = run(
        &d,
        TOGGLES,
        &["--code-coverage=toggle", "--no-toggle-enums"],
    );
    assert_eq!(
        strings(scope(&enums, "tb"), "signal"),
        [
            "clk",
            "genblk_status",
            "genblk_status.m",
            "o",
            "us",
            "us.k",
            "us.m"
        ]
    );
    let generate = run(
        &d,
        TOGGLES,
        &["--code-coverage=toggle", "--no-toggle-generate"],
    );
    assert_eq!(
        strings(scope(&generate, "tb"), "signal"),
        [
            "clk",
            "genblk_status",
            "genblk_status.m",
            "o",
            "st",
            "us",
            "us.k",
            "us.m"
        ]
    );
    assert_eq!(
        strings(scope(&generate, "tb.u"), "signal"),
        ["clk", "o", "r"]
    );
    assert_eq!(strings(scope(&generate, "tb.w"), "signal"), ["clk", "keep"]);
    let _ = std::fs::remove_dir_all(&d);
}

const ITEMS: &str = "\
module m(input logic clk, input logic a, b, c, input logic [1:0] v, output logic y, z);
  logic [7:0] w = 8'h0F;
  int k = 0;
  assign y = a && b;
  always @(posedge clk) begin
    if (a && b) z <= 1;
    if (a) z <= 0;
    if (v == 2'd1) z <= 1;
    if ((a & b) || c) z <= 0;
    if (!a && v) z <= 1;
    if (!(a && b)) z <= 0;
    if (b && a) z <= 1;
    if (b && ok(a)) z <= 0;
    if (b && w[k*$clog2(4) +: 2] == 2'd3) z <= 1;
  end
  function automatic logic ok(input logic x);
    return x;
  endfunction
endmodule
module tb;
  logic clk = 0, a = 0, b = 0, c = 0; logic [1:0] v = 0; wire y, z;
  m u(.clk, .a, .b, .c, .v, .y, .z);
  always #5 clk = ~clk;
  initial begin #17 a = 1; b = 0; #10 c = 1; v = 2; #10 $finish; end
endmodule
";

/// `--code-coverage-expressions=logical` scores only the conditions that
/// hold a `&&` or `||`, with their operands as terms: `(a & b) || c` has
/// the terms `a & b` and `c`, `!a && v` has `!a` and `v`, and `!(a && b)`
/// keeps `a` and `b` under its `!`. A condition
/// without one (`if (a)`, `if (v == 2'd1)`) and an assignment's value
/// (`assign y = a && b`) are no items. A term is covered once it has been
/// 0 and 1, whether or not it decided the value: `a` of `a && b` is 1 only
/// while `b` is 0, which the default rules (the term must decide) don't
/// count. A term a short circuit skips is read all the same when reading it
/// has no side effects (`a` of `b && a`), and not otherwise (`ok(a)`).
#[test]
fn logical_conditions_scored_by_term_values() {
    let d = scratch("logical");
    let all = run(&d, ITEMS, &["--code-coverage=cond,expr"]);
    let u = scope(&all, "tb.u");
    assert_eq!(
        strings(u, "text"),
        [
            "a && b",
            "v == 2'd1",
            "(a & b) || c",
            "!a && v",
            "!(a && b)",
            "b && a",
            "b && ok(a)",
            "b && w[k*$clog2(4) +: 2] == 2'd3",
            "a && b"
        ]
    );
    assert!(u.contains("{\"term\": \"a\", \"hits\": [2, 0]}"), "{u}");
    let logical = run(
        &d,
        ITEMS,
        &[
            "--code-coverage=cond,expr",
            "--code-coverage-expressions=logical",
        ],
    );
    let u = scope(&logical, "tb.u");
    assert_eq!(
        strings(u, "text"),
        [
            "a && b",
            "(a & b) || c",
            "!a && v",
            "!(a && b)",
            "b && a",
            "b && ok(a)",
            "b && w[k*$clog2(4) +: 2] == 2'd3"
        ]
    );
    assert_eq!(
        strings(u, "term"),
        [
            "a",
            "b",
            "a & b",
            "c",
            "!a",
            "v",
            "a",
            "b",
            "b",
            "a",
            "b",
            "ok(a)",
            "b",
            "w[k*$clog2(4) +: 2] == 2'd3"
        ]
    );
    // The item's text and its terms.
    let item = |text: &str| {
        let at = u.find(&format!("\"text\": \"{text}\"")).unwrap();
        &u[at..at + u[at..].find("]}]}").unwrap() + 4]
    };
    assert!(
        item("a && b").contains("{\"term\": \"a\", \"hits\": [2, 2]}"),
        "{u}"
    );
    // A short circuit doesn't hide a side-effect-free term (`a`, read while
    // `b` is 0, and a comparison of a part-select whose index holds a
    // `$clog2`), but a function call is not read again.
    assert!(
        item("b && w[k*$clog2(4) +: 2] == 2'd3")
            .contains("{\"term\": \"w[k*$clog2(4) +: 2] == 2'd3\", \"hits\": [0, 4]}"),
        "{u}"
    );
    assert!(
        item("b && a").contains("{\"term\": \"a\", \"hits\": [2, 2]}"),
        "{u}"
    );
    assert!(
        item("b && ok(a)").contains("{\"term\": \"ok(a)\", \"hits\": [0, 0]}"),
        "{u}"
    );
    assert!(
        u.contains("\"condition\": {\"covered\": 6, \"total\": 14, \"percent\": 42.86}"),
        "{u}"
    );
    assert!(
        u.contains("\"expression\": {\"covered\": 0, \"total\": 0"),
        "{u}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

const BLOCKS: &str = "\
module m(input logic clk, input logic rst, input logic [1:0] s, output logic [3:0] q);
  logic x, y, z;
  assign z = x ^ y;
  always @(posedge clk) begin
    x <= 1;
    if (rst) q <= 0;
    else begin
      q <= q + 1;
      case (s)
        2'd0: q <= 1;
        2'd1: q <= 2;
      endcase
    end
    y <= x;
  end
  always @(posedge clk)
    if (s == 2'd3) x <= 0;
  initial begin
    for (int i = 0; i < 2; i++) begin
      y = i[0];
    end
    #1 y = 0;
    z = 1;
  end
endmodule
module tb;
  logic clk = 0, rst = 1; logic [1:0] s = 0; wire [3:0] q;
  m u(.clk, .rst, .s, .q);
  always #5 clk = ~clk;
  initial begin #12 rst = 0; s = 0; #10 s = 1; #10 s = 2; #10 $finish; end
endmodule
";

/// `--code-coverage-granularity=block` counts straight-line blocks, each
/// at its first statement: a process body up to its first branch (5), each
/// arm (6, 8, 10, 11), the code after a branch (14) or a loop, a loop body
/// (20) and the code after a delay (22, with 23). An implicit `else` or
/// `default` is a block of its own (9, 17), run when no written arm is. A
/// continuous assignment (3) and the `always` construct itself are no
/// blocks. Branch coverage or not, the blocks are the same.
#[test]
fn block_granularity() {
    let d = scratch("blocks");
    let rows = |json: &str| -> Vec<(u32, String)> {
        scope(json, "tb.u")
            .split("{\"file\": \"t.sv\", \"line\": ")
            .skip(1)
            .take_while(|r| r.contains("\"count\"") && !r.contains("\"arms\""))
            .map(|r| {
                let line = r[..r.find(',').unwrap()].parse().unwrap();
                (line, strings(r, "kind")[0].to_string())
            })
            .collect()
    };
    let json = run(
        &d,
        BLOCKS,
        &[
            "--code-coverage=stmt,branch",
            "--code-coverage-granularity=block",
        ],
    );
    let u = scope(&json, "tb.u");
    let expected: Vec<(u32, String)> = [
        (5, "block"),
        (6, "block"),
        (8, "block"),
        (9, "default (implicit)"),
        (10, "block"),
        (11, "block"),
        (14, "block"),
        (17, "block"),
        (17, "else (implicit)"),
        (17, "block"),
        (19, "block"),
        (20, "block"),
        (22, "block"),
    ]
    .into_iter()
    .map(|(l, k)| (l, k.to_string()))
    .collect();
    assert_eq!(rows(&json), expected, "{u}");
    assert!(
        u.contains("\"statement\": {\"covered\": 12, \"total\": 13, \"percent\": 92.31}"),
        "{u}"
    );
    let stmts = run(
        &d,
        BLOCKS,
        &["--code-coverage=stmt", "--code-coverage-granularity=block"],
    );
    assert_eq!(rows(&stmts), expected, "{stmts}");
    let _ = std::fs::remove_dir_all(&d);
}

const FSM: &str = "\
module e(input logic clk, input logic en, input logic go);
  typedef enum logic [1:0] {IDLE, RUN, DONE} st_t;
  st_t st = IDLE;
  always @(posedge clk)
    if (en)
      case (st)
        IDLE: if (go) st <= RUN;
        RUN: st <= DONE;
        DONE: st <= IDLE;
      endcase
endmodule
module m(input logic clk, input logic rst, input logic go, output logic busy);
  typedef enum logic [1:0] {IDLE, RUN, DONE} st_t;
  st_t st;
  always @(posedge clk or posedge rst)
    if (rst) st <= IDLE;
    else case (st)
      IDLE: if (go) st <= RUN;
      RUN: st <= DONE;
      DONE: if (!go) st <= RUN;
    endcase
  assign busy = st != IDLE;
endmodule
module a(input logic clk, input logic reset, input logic go);
  localparam [1:0] IDLE = 2'd0, RUN = 2'd1, DONE = 2'd2;
  logic [1:0] st;
  always @(posedge clk or posedge reset) begin
    logic [1:0] prev;
    if (reset) st <= IDLE;
    else case (st)
      IDLE: if (go) st <= RUN;
      RUN: st <= DONE;
      DONE: if (!go) st <= RUN;
    endcase
  end
endmodule
module b(input logic clk, input logic rst_n, input logic go, output logic seen);
  typedef enum logic [1:0] {IDLE, RUN, DONE} st_t;
  st_t st;
  always @(posedge clk or negedge rst_n) begin
    seen <= go;
    if (!rst_n) st <= IDLE;
    else case (st)
      IDLE: if (go) st <= RUN;
      RUN: st <= DONE;
      DONE: if (!go) st <= RUN;
    endcase
  end
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0; wire busy;
  m u(.clk, .rst, .go, .busy);
  e g(.clk, .en(go), .go);
  a ua(.clk, .reset(rst), .go);
  b ub(.clk, .rst_n(!rst), .go, .seen());
  always #5 clk = ~clk;
  initial begin #12 rst = 0; go = 1; #30 go = 0; #20 rst = 1; #10 rst = 0; #20 $finish; end
endmodule
";

/// `--no-fsm-reset-transitions` leaves out the transitions that only the
/// state process's reset branch makes (`RUN -> IDLE`, `DONE -> IDLE`); the
/// others stay. With an asynchronous reset, the `if` testing it is the reset
/// branch, past a declaration (`a`) or another statement (`b`) in front of
/// it. A clock enable (`if (en) case (st)`) is no reset: its FSM keeps every
/// transition. A state that stays put is never a transition.
#[test]
fn fsm_reset_transitions_left_out() {
    let d = scratch("fsm");
    let all = run(&d, FSM, &["--code-coverage=fsm"]);
    let pairs = |json: &str, inst: &str| {
        let u = scope(json, inst);
        strings(u, "from")
            .into_iter()
            .zip(strings(u, "to"))
            .map(|(f, t)| format!("{f}->{t}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        pairs(&all, "tb.u"),
        [
            "IDLE->RUN",
            "RUN->IDLE",
            "RUN->DONE",
            "DONE->IDLE",
            "DONE->RUN"
        ]
    );
    let logic = run(
        &d,
        FSM,
        &["--code-coverage=fsm", "--no-fsm-reset-transitions"],
    );
    assert_eq!(
        pairs(&logic, "tb.u"),
        ["IDLE->RUN", "RUN->DONE", "DONE->RUN"]
    );
    for inst in ["tb.ua", "tb.ub"] {
        assert_eq!(pairs(&all, inst), pairs(&all, "tb.u"), "{inst}");
        assert_eq!(
            pairs(&logic, inst),
            ["IDLE->RUN", "RUN->DONE", "DONE->RUN"],
            "{inst}"
        );
    }
    let enabled = ["IDLE->RUN", "RUN->DONE", "DONE->IDLE"];
    assert_eq!(pairs(&all, "tb.g"), enabled);
    assert_eq!(pairs(&logic, "tb.g"), enabled);
    let _ = std::fs::remove_dir_all(&d);
}

const DEAD_FSM: &str = "\
module p #(parameter bit ON = 1'b0) (input logic clk, input logic rst, input logic go);
  typedef enum logic [1:0] {IDLE, RUN, DONE, HALT} st_t;
  st_t st;
  always @(posedge clk or posedge rst)
    if (rst) st <= IDLE;
    else case (st)
      IDLE: if (go) st <= RUN;
      RUN: if (ON && go) st <= HALT;
           else st <= DONE;
      DONE: st <= IDLE;
      default: ;
    endcase
endmodule
module t #(parameter bit ON = 1'b0) (input logic clk, input logic rst, input logic go);
  typedef enum logic [1:0] {IDLE, RUN, DONE, HALT} st_t;
  st_t st;
  always @(posedge clk or posedge rst)
    if (rst) st <= IDLE;
    else case (st)
      IDLE: if (go) st <= RUN;
      RUN: st <= (go & ON) ? HALT : DONE;
      DONE: st <= IDLE;
      default: ;
    endcase
endmodule
module q #(parameter bit ON = 1'b0) (input logic clk, input logic rst, input logic go);
  typedef enum logic [1:0] {IDLE, RUN, DONE} st_t;
  st_t st;
  always @(posedge clk)
    if (!ON || rst) st <= IDLE;
    else case (st)
      IDLE: if (go) st <= RUN;
      RUN: st <= DONE;
      DONE: st <= IDLE;
    endcase
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0;
  p #(.ON(1'b0)) u0(.clk, .rst, .go);
  p #(.ON(1'b1)) u1(.clk, .rst, .go);
  t #(.ON(1'b0)) t0(.clk, .rst, .go);
  q #(.ON(1'b0)) v(.clk, .rst, .go);
  always #5 clk = ~clk;
  initial begin #12 rst = 0; go = 1; #60 go = 0; #30 $finish; end
endmodule
";

/// Code a constant rules out (see Constant conditions) holds no FSM states
/// or transitions, also where the rest of a condition can't change its
/// value: with `ON` 0, `if (ON && go)` and `(go & ON) ? HALT : DONE` never
/// make `HALT`, and `if (!ON || rst)` leaves `q`'s state logic out, so `q`
/// has no FSM at all. With `ON` 1 the same code counts.
#[test]
fn fsm_in_constant_dead_code_left_out() {
    let d = scratch("deadfsm");
    let json = run(&d, DEAD_FSM, &["--code-coverage=fsm,stmt"]);
    let fsm = |inst: &str| {
        let u = scope(&json, inst);
        let states = u
            .find("\"states\": [")
            .map(|a| &u[a..a + u[a..].find(']').unwrap()])
            .unwrap_or("");
        let pairs: Vec<String> = strings(u, "from")
            .into_iter()
            .zip(strings(u, "to"))
            .map(|(f, t)| format!("{f}->{t}"))
            .collect();
        (strings(states, "name"), pairs)
    };
    for inst in ["tb.u0", "tb.t0"] {
        let (states, pairs) = fsm(inst);
        assert_eq!(states, ["IDLE", "RUN", "DONE"], "{inst}: {json}");
        assert_eq!(
            pairs,
            ["IDLE->RUN", "RUN->IDLE", "RUN->DONE", "DONE->IDLE"],
            "{inst}: {json}"
        );
    }
    let (states, pairs) = fsm("tb.u1");
    assert_eq!(states, ["IDLE", "RUN", "DONE", "HALT"], "{json}");
    assert_eq!(
        pairs,
        [
            "IDLE->RUN",
            "RUN->IDLE",
            "RUN->DONE",
            "RUN->HALT",
            "DONE->IDLE",
            "HALT->IDLE"
        ],
        "{json}"
    );
    assert!(!scope(&json, "tb.v").contains("\"variable\""), "{json}");
    let _ = std::fs::remove_dir_all(&d);
}

const TIED: &str = "\
module fa(input logic a, input logic b, input logic cin, output logic s, output logic co);
  assign s = a ^ b ^ cin;
  assign co = (a & b) | (b & cin) | (a & cin);
endmodule
module sub(input logic [3:0] a, input logic c, output logic [3:0] y, output logic [7:0] w);
  wire [7:0] z = {4'b0000, a};
  wire [11:4] n;
  wire [0:3] asc;
  assign n[11:8] = 4'd0;
  assign n[7:4] = a;
  assign asc[0:1] = a[1:0];
  assign asc[2:3] = 2'b00;
  assign w = z;
  assign y = {1'b0, a[2:1], c};
endmodule
module tb;
  localparam logic P = 1'b1;
  logic [3:0] a = 0;
  logic b = 0;
  wire [3:0] y;
  wire [7:0] w;
  wire s, co, q, k;
  sub u(.a(a), .c(1'b0), .y(y), .w(w));
  fa f(.a(b), .b(P), .cin(1'b0), .s(s), .co(co));
  assign q = 1'b0;
  assign k = 1'b1;
  initial begin
    #1 force k = 1'b0;
    repeat (6) begin #5 a = (a + 1) % 4; b = ~b; end
    $finish;
  end
endmodule
";

/// Each toggle entry of `scope` that has tied bits: `signal: [bits]`.
fn tied(json: &str, scope_name: &str) -> Vec<String> {
    scope(json, scope_name)
        .split("{\"signal\": \"")
        .skip(1)
        .filter_map(|e| {
            let e = &e[..e.find('}').unwrap()];
            let at = e.find("\"tied\": [")? + "\"tied\": [".len();
            let name = &e[..e.find('"').unwrap()];
            Some(format!("{name}: [{}]", &e[at..at + e[at..].find(']').unwrap()]))
        })
        .collect()
}

/// `--no-toggle-tied` leaves out the toggle bins of the bits whose only
/// drivers are constants: literal and parameter bits of a continuous
/// assignment or port connection (`z`, `n`, `asc`, `y[3]`, `u.c`, `f.b`,
/// `f.cin`, `q`), and the bits wired to such a bit (`w`, `y[0]`, the
/// parent's `y` and `w`). An operator on a constant (`f.co`) ties nothing,
/// nor does a value range (`a[3:2]` stay 0), nor a net a `force` writes
/// (`k`). The bits are listed by position in the vector, least significant
/// first, whichever way its range runs.
#[test]
fn toggle_tied_bits_left_out() {
    let d = scratch("tied");
    let all = run(&d, TIED, &["--code-coverage=toggle"]);
    assert!(!all.contains("\"tied\""), "{all}");
    let left = run(&d, TIED, &["--code-coverage=toggle", "--no-toggle-tied"]);
    assert_eq!(
        tied(&left, "tb.u"),
        [
            "asc: [0, 1]",
            "c: [0]",
            "n: [4, 5, 6, 7]",
            "w: [4, 5, 6, 7]",
            "y: [0, 3]",
            "z: [4, 5, 6, 7]"
        ],
        "{left}"
    );
    assert_eq!(tied(&left, "tb.f"), ["b: [0]", "cin: [0]"], "{left}");
    assert_eq!(
        tied(&left, "tb"),
        ["q: [0]", "w: [4, 5, 6, 7]", "y: [0, 3]"],
        "{left}"
    );
    assert!(scope(&left, "tb.f").contains("\"excluded\": 4}"), "{left}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Each switch is also an environment variable, which the flag overrides.
#[test]
fn rules_from_the_environment() {
    let d = scratch("env");
    std::fs::write(d.join("t.sv"), TOGGLES).unwrap();
    let out = Command::new(xezim())
        .current_dir(&d)
        .env("XEZIM_COV_DB", d.join("cov.json"))
        .env("XEZIM_TOGGLE_ENUMS", "0")
        .env("XEZIM_TOGGLE_GENERATE", "0")
        .args(["--code-coverage=toggle", "--toggle-enums", "t.sv"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let json = std::fs::read_to_string(d.join("cov.json")).unwrap();
    assert!(
        strings(scope(&json, "tb"), "signal").contains(&"st"),
        "{json}"
    );
    assert_eq!(strings(scope(&json, "tb.u"), "signal"), ["clk", "o", "r"]);
    let bad = Command::new(xezim())
        .current_dir(&d)
        .env("XEZIM_CODE_COVERAGE_GRANULARITY", "line")
        .args(["--code-coverage", "t.sv"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("want statement or block"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    let _ = std::fs::remove_dir_all(&d);
}
