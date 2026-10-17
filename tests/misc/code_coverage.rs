//! Code coverage (`--code-coverage`, docs/coverage-guide.md): statement,
//! branch and toggle counts in xezim_cov.json, per the guide's counting
//! rules. Where a rule matches the reference simulator's report on the same
//! design, the test says so.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn xezim() -> String {
    let mut p = std::env::current_exe().expect("current_exe");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("xezim").to_string_lossy().into_owned()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xezim_code_cov_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run xezim on `src` (written to `t.sv` in `dir`) with `args`; the results
/// file is `cov.json` there.
fn run(dir: &Path, src: &str, args: &[&str]) -> Output {
    std::fs::write(dir.join("t.sv"), src).unwrap();
    Command::new(xezim())
        .current_dir(dir)
        .env("XEZIM_COV_DB", dir.join("cov.json"))
        .env_remove("XEZIM_CODE_COVERAGE")
        .env_remove("XEZIM_TOGGLE_INPUT_PORTS")
        .env_remove("XEZIM_VERBOSE")
        .args(args)
        .arg("t.sv")
        .output()
        .expect("run xezim")
}

fn results(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("cov.json")).unwrap_or_default()
}

fn stmt(line: u32, kind: &str, count: u64) -> String {
    format!(
        "{{\"file\": \"t.sv\", \"line\": {}, \"kind\": \"{}\", \"count\": {}}}",
        line, kind, count
    )
}

/// The counts of the arms of the branch at `line` of kind `kind`.
fn arm_counts(json: &str, line: u32, kind: &str) -> Vec<u64> {
    let key = format!("\"line\": {}, \"kind\": \"{}\", \"arms\": [", line, kind);
    let start = json
        .find(&key)
        .unwrap_or_else(|| panic!("no {kind} at {line}: {json}"));
    let rest = &json[start + key.len()..];
    let arms = &rest[..rest.find("]}").unwrap()];
    arms.split("\"count\": ")
        .skip(1)
        .map(|c| {
            c.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap()
        })
        .collect()
}

const DESIGN: &str = "\
module sub(input logic clk, input logic [1:0] sel, output logic [3:0] y);
  logic [3:0] r;
  assign y = r;
  always @(posedge clk) begin
    case (sel)
      2'd0: r <= 4'h1;
      2'd1: r <= 4'h2;
      2'd2: r <= 4'h4;
    endcase
  end
endmodule
module tb;
  logic clk = 0;
  logic [1:0] sel;
  logic [3:0] y0;
  int cnt;
  sub u0(.clk(clk), .sel(sel), .y(y0));
  always #5 clk = ~clk;
  function int f(int x);
    if (x > 2)
      return 1;
    return 0;
  endfunction
  task t(input int n);
    for (int i = 0; i < n; i++)
      cnt++;
  endtask
  initial begin
    sel = 0; cnt = 0;
    if (cnt == 5) cnt = 2; else cnt = 3;
    t(3);
    cnt = f(cnt) ? 7 : 8;
    unique case (cnt)
      7: cnt = 1;
      8: cnt = 2;
    endcase
    @(negedge clk) sel = 1;
    @(negedge clk) sel = 3;
    @(negedge clk) sel = 2;
    @(negedge clk);
    $finish;
  end
endmodule
";

/// Statements count each time they start: leaves, `always` constructs,
/// timing controls and loops (once per start), continuous assignments; `if`
/// and `case` are branches, not statements. The flop's statements and the
/// function's `return`s have the reference simulator's counts.
#[test]
fn statement_counts() {
    let d = scratch("stmt");
    let out = run(&d, DESIGN, &["--code-coverage=stmt"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    assert!(json.contains("\"kinds\": [\"statement\"]"), "{json}");
    assert!(
        !json.contains("\"branches\"") && !json.contains("\"toggles\""),
        "{json}"
    );
    for (line, kind, count) in [
        (18, "always", 8),
        (18, "statement", 8),
        (21, "statement", 1),
        (22, "statement", 0),
        (25, "for", 1),
        (26, "statement", 3),
        (30, "statement", 0),
        (30, "statement", 1),
        (32, "statement", 1),
        (34, "statement", 1),
        (35, "statement", 0),
        (37, "event", 1),
        (37, "statement", 1),
        (40, "event", 1),
        (41, "statement", 1),
        (4, "always", 4),
        (6, "statement", 1),
        (7, "statement", 1),
        (8, "statement", 1),
    ] {
        assert!(
            json.contains(&stmt(line, kind, count)),
            "line {line}: {json}"
        );
    }
    assert!(json.contains("\"line\": 3, \"kind\": \"assign\""), "{json}");
    for line in [5, 20, 33] {
        assert!(
            !json.contains(&format!("\"line\": {line}, \"kind\"")),
            "{json}"
        );
    }
    assert!(
        json.contains("\"scope\": \"tb.u0\",\n        \"design_unit\": \"sub\""),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Branch arms: `if` with its implicit `else`, `if`/`else`, `?:`, a `case`
/// without `default` (an implicit arm) and a `unique case` (none). The
/// counts match the reference simulator's report for the same design.
#[test]
fn branch_arms() {
    let d = scratch("branch");
    let out = run(&d, DESIGN, &["--code-coverage=branch"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 20, \"kind\": \"if\", \"arms\": [\
         {\"line\": 21, \"arm\": \"if\", \"count\": 1}, \
         {\"line\": 20, \"arm\": \"else (implicit)\", \"count\": 0}]}"
        ),
        "{json}"
    );
    assert_eq!(arm_counts(&json, 30, "if"), [0, 1]);
    assert_eq!(arm_counts(&json, 32, "ternary"), [1, 0]);
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 33, \"kind\": \"case\", \"arms\": [\
         {\"line\": 34, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 35, \"arm\": \"item\", \"count\": 0}]}"
        ),
        "{json}"
    );
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 5, \"kind\": \"case\", \"arms\": [\
         {\"line\": 6, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 7, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 8, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 5, \"arm\": \"default (implicit)\", \"count\": 1}]}"
        ),
        "{json}"
    );
    assert!(
        json.contains("\"branch\": {\"covered\": 8, \"total\": 12, \"percent\": 66.67}"),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// An `else if` chain is one branch, and a `?:` in a continuous assignment
/// is a branch too (combinational counts are evaluations, so only which arms
/// were taken is checked).
#[test]
fn branch_chain_and_assign_ternary() {
    let d = scratch("chain");
    let src = "\
module tb;
  logic a, b, c;
  logic [1:0] k;
  wire w = a ? b : c;
  always @(a, b)
    if (a)
      k = 1;
    else if (b)
      k = 2;
    else
      k = 3;
  initial begin
    a = 0; b = 0; c = 0;
    #1 b = 1;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=branch"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let chain = arm_counts(&json, 6, "if");
    assert_eq!(chain.len(), 3, "{json}");
    assert!(chain[0] == 0 && chain[1] > 0 && chain[2] > 0, "{json}");
    let w = arm_counts(&json, 4, "ternary");
    assert!(w[0] == 0 && w[1] > 0, "{json}");
    assert!(json.contains("\"arm\": \"else if\""), "{json}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Toggles: per bit, 0->1 and 1->0 between the ends of time slots, never to
/// or from x; memories and reals untracked, and input ports too under
/// `--no-toggle-input-ports`. As in the reference simulator's report, a
/// 0-1-0 within one time slot is no toggle.
#[test]
fn toggle_counts() {
    let d = scratch("toggle");
    let src = "\
module leaf(input logic a, output logic o);
  assign o = ~a;
endmodule
module tb;
  logic x, g;
  logic [1:0] v;
  logic [7:0] mem [4];
  int unsigned n;
  real r;
  leaf u(.a(x), .o());
  initial begin
    x = 0; g = 0; v = 0; n = 0; r = 0.5;
    #1 x = 1;
    #1 g = 1; g = 0;
    #1 v = 2'b10; v = 2'b01;
    #1 v = 2'bx1;
    #1 x = 0;
    mem[0] = 8'hff;
    n = 3;
  end
endmodule
";
    let out = run(
        &d,
        src,
        &["--code-coverage=toggle", "--no-toggle-input-ports"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    for row in [
        "{\"signal\": \"x\", \"width\": 1, \"rise\": [1], \"fall\": [1]}",
        "{\"signal\": \"g\", \"width\": 1, \"rise\": [0], \"fall\": [0]}",
        "{\"signal\": \"v\", \"width\": 2, \"rise\": [1, 0], \"fall\": [0, 0]}",
        "{\"signal\": \"o\", \"width\": 1, \"rise\": [1], \"fall\": [1]}",
    ] {
        assert!(json.contains(row), "{row}: {json}");
    }
    assert!(
        json.contains("{\"signal\": \"n\", \"width\": 32, \"rise\": [1, 1, 0,"),
        "{json}"
    );
    for name in ["mem", "r", "a", "u"] {
        assert!(
            !json.contains(&format!("\"signal\": \"{name}\"")),
            "{name}: {json}"
        );
    }
    assert!(
        json.contains("\"toggle\": {\"covered\": 7, \"total\": 74, \"percent\": 9.46}"),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The entry of scope `name` in the results file, up to the next scope's.
fn scope<'a>(json: &'a str, name: &str) -> &'a str {
    let key = format!("\"scope\": \"{}\",", name);
    let start = json
        .find(&key)
        .unwrap_or_else(|| panic!("no scope {name}: {json}"));
    let rest = &json[start + key.len()..];
    let end = rest
        .find("\"scope\": ")
        .or_else(|| rest.find("\"design_units\""))
        .unwrap_or(rest.len());
    &json[start..start + key.len() + end]
}

fn toggle(signal: &str, rise: &[u32], fall: &[u32]) -> String {
    format!(
        "{{\"signal\": \"{}\", \"width\": {}, \"rise\": {:?}, \"fall\": {:?}}}",
        signal,
        rise.len(),
        rise,
        fall
    )
}

fn toggle_total(covered: u32, total: u32, percent: &str) -> String {
    format!(
        "\"toggle\": {{\"covered\": {}, \"total\": {}, \"percent\": {}}}",
        covered, total, percent
    )
}

/// A child with one input port per kind of connection, an inout, an
/// unpacked port array and an interface port.
const PORTS: &str = "\
interface bus_if;
  logic [1:0] req;
  modport mp (input req);
endinterface
module leaf (input logic [1:0] l, output logic [1:0] q);
  assign q = l;
endmodule
module child (
  input  logic       clk,
  input  logic [3:0] a,
  input  logic [3:0] b,
  input  logic [3:0] c,
  input  logic [3:0] d,
  input  logic [1:0] e,
  input  logic [3:0] f,
  input  logic [3:0] g,
  input  logic [1:0] arr [2],
  inout  wire  [1:0] io,
  bus_if.mp          bi,
  output logic [3:0] y
);
  logic [1:0] lq;
  leaf u_leaf (.l(e), .q(lq));
  always_ff @(posedge clk)
    y <= a ^ b ^ c ^ {e, e} ^ f ^ g ^ {arr[0], arr[1]} ^ {io, io} ^ {bi.req, bi.req};
endmodule
module tb;
  logic clk = 0;
  logic [3:0] a = 0;
  logic [1:0] n = 0;
  logic [1:0] m [2];
  logic [1:0] drv = 0;
  wire  [1:0] io;
  logic [3:0] y;
  assign io = drv;
  bus_if bif();
  child u (.clk, .a(a), .b(~a), .c(4'd5), .d(), .e(a[2:1]), .f({n, a[1:0]}),
           .g(n), .arr(m), .io(io), .bi(bif.mp), .y);
  always #5 clk = ~clk;
  initial begin
    m[0] = 0; m[1] = 0; bif.req = 0;
    #12 a = 4'b0011; n = 2'b01; drv = 2'b01; m[0] = 2'b11; bif.req = 2'b01;
    #10 a = 4'b0000; n = 2'b00; drv = 2'b00; m[0] = 2'b00; bif.req = 2'b00;
    #10 a = 4'b1000;
    #10 $finish;
  end
endmodule
";

/// §23.3.3: a port connection is a continuous assignment into the port's
/// own net, so an instance's toggle coverage counts its input and inout
/// ports like its outputs, with the value the connection gives them: a
/// whole net's counts (`a`, `clk`, `io`, and `u_leaf.l` through the
/// parent's port `e`), an expression's (`b = ~a`), a select's or
/// concatenation's (`e`, `f`), a narrower actual's low bit (`g`). A constant
/// (`c`) or open (`d`) port never toggles. The unpacked port array and the
/// interface port are not tracked. Which bins are hit, and every scope's
/// totals, are the reference simulator's with full signal visibility.
#[test]
fn input_ports_toggle_in_their_instance() {
    let d = scratch("ports");
    let out = run(&d, PORTS, &["--code-coverage=toggle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let u = scope(&json, "tb.u");
    for row in [
        toggle("a", &[1, 1, 0, 1], &[1, 1, 0, 0]),
        toggle("b", &[1, 1, 0, 0], &[1, 1, 0, 1]),
        toggle("c", &[0, 0, 0, 0], &[0, 0, 0, 0]),
        toggle("clk", &[4], &[4]),
        toggle("d", &[0, 0, 0, 0], &[0, 0, 0, 0]),
        toggle("e", &[1, 0], &[1, 0]),
        toggle("f", &[1, 1, 1, 0], &[1, 1, 1, 0]),
        toggle("g", &[1, 0, 0, 0], &[1, 0, 0, 0]),
        toggle("io", &[1, 0], &[1, 0]),
        toggle("y", &[1, 1, 1, 1], &[1, 1, 1, 1]),
    ] {
        assert!(u.contains(&row), "{row}: {u}");
    }
    for name in ["arr", "bi"] {
        assert!(!u.contains(&format!("\"signal\": \"{name}")), "{name}: {u}");
    }
    assert!(u.contains(&toggle_total(34, 70, "48.57")), "{u}");
    let leaf = scope(&json, "tb.u.u_leaf");
    assert!(leaf.contains(&toggle("l", &[1, 0], &[1, 0])), "{leaf}");
    assert!(leaf.contains(&toggle_total(4, 8, "50.00")), "{leaf}");
    assert!(
        scope(&json, "tb").contains(&toggle_total(21, 30, "70.00")),
        "{json}"
    );
    assert!(
        scope(&json, "tb.bif").contains(&toggle_total(2, 4, "50.00")),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// More port shapes, against the same reference report: an interface's own
/// input ports count in the interface instance (`id` is tied to a
/// constant), each element of an instance array has its slice of the
/// connection, and packed-array, packed-struct, truncated and net ports
/// toggle bit by bit.
#[test]
fn port_shapes_toggle_in_their_instance() {
    let d = scratch("port_shapes");
    let src = "\
typedef struct packed { logic [1:0] hi; logic lo; } st_t;
interface clk_if (input logic clk, input logic [1:0] id);
  logic v = 0;
  always @(posedge clk) v <= ~v;
endinterface
module leaf (input logic [1:0] l, output logic [1:0] q);
  assign q = l;
endmodule
module pk (input logic [1:0][1:0] p2, input st_t s, input logic [1:0] t,
           input wire [1:0] wn, output logic o);
  assign o = ^p2 ^ ^s ^ ^t ^ ^wn;
endmodule
module tb;
  logic clk = 0;
  logic [3:0] bus = 0;
  logic [1:0][1:0] pp = 0;
  st_t ss = 0;
  logic [3:0] wide = 0;
  wire [1:0] net;
  logic [3:0] qa;
  logic o;
  assign net = bus[3:2];
  clk_if ci (.clk(clk), .id(2'd2));
  leaf ua [1:0] (.l(bus), .q(qa));
  pk up (.p2(pp), .s(ss), .t(wide), .wn(net), .o);
  always #5 clk = ~clk;
  initial begin
    #12 bus = 4'b0110; pp = 4'b1001; ss = 3'b101; wide = 4'b1110;
    #10 bus = 4'b1100; pp = 4'b0000; ss = 3'b000; wide = 4'b0000;
    #10 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=toggle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let expect: [(&str, &[String], (u32, u32, &str)); 4] = [
        (
            "tb.ci",
            &[toggle("clk", &[3], &[3]), toggle("id", &[0, 0], &[0, 0])],
            (4, 8, "50.00"),
        ),
        (
            "tb.ua[0]",
            &[toggle("l", &[0, 1], &[0, 1])],
            (4, 8, "50.00"),
        ),
        (
            "tb.ua[1]",
            &[toggle("l", &[1, 1], &[0, 0])],
            (4, 8, "50.00"),
        ),
        (
            "tb.up",
            &[
                toggle("p2", &[1, 0, 0, 1], &[1, 0, 0, 1]),
                toggle("s", &[1, 0, 1], &[1, 0, 1]),
                toggle("t", &[0, 1], &[0, 1]),
                toggle("wn", &[1, 1], &[0, 0]),
            ],
            (12, 24, "50.00"),
        ),
    ];
    for (name, rows, (covered, total, percent)) in expect {
        let s = scope(&json, name);
        for row in rows {
            assert!(s.contains(row.as_str()), "{row}: {s}");
        }
        assert!(s.contains(&toggle_total(covered, total, percent)), "{s}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `--no-toggle-input-ports` and XEZIM_TOGGLE_INPUT_PORTS=0 leave input and
/// inout ports to the signal that drives them, as before they were counted;
/// `--toggle-input-ports` wins over the variable.
#[test]
fn input_ports_opt_out() {
    let d = scratch("ports_off");
    std::fs::write(d.join("t.sv"), PORTS).unwrap();
    let child = |args: &[&str], var: Option<&str>| {
        let mut cmd = Command::new(xezim());
        cmd.current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .env_remove("XEZIM_CODE_COVERAGE")
            .env_remove("XEZIM_TOGGLE_INPUT_PORTS");
        if let Some(v) = var {
            cmd.env("XEZIM_TOGGLE_INPUT_PORTS", v);
        }
        let out = cmd
            .arg("--code-coverage=toggle")
            .args(args)
            .arg("t.sv")
            .output()
            .expect("run xezim");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        scope(&results(&d), "tb.u").to_string()
    };
    let off = child(&["--no-toggle-input-ports"], None);
    for name in ["a", "b", "c", "clk", "d", "e", "f", "g", "io"] {
        assert!(
            !off.contains(&format!("\"signal\": \"{name}\"")),
            "{name}: {off}"
        );
    }
    // The output `y` and the net `lq` are left.
    assert!(off.contains(&toggle_total(10, 12, "83.33")), "{off}");
    assert_eq!(child(&[], Some("0")), off);
    let on = toggle_total(34, 70, "48.57");
    assert!(child(&[], Some("1")).contains(&on));
    assert!(child(&["--toggle-input-ports"], Some("0")).contains(&on));
    let _ = std::fs::remove_dir_all(&d);
}

/// The two-state executors take back a count when they hand a block to the
/// four-state VM, so both give the same counts (the flop reads x before
/// `d` is driven).
#[test]
fn counts_do_not_depend_on_executor() {
    let d = scratch("engine");
    let src = "\
module tb;
  logic clk = 0;
  logic [3:0] q, d;
  always #5 clk = ~clk;
  always_ff @(posedge clk)
    if (d[0]) q <= d;
    else q <= ~d;
  initial begin
    #12 d = 4'h5;
    #20 d = 4'h2;
    #20 $finish;
  end
endmodule
";
    let code = |extra: &[(&str, &str)]| {
        std::fs::write(d.join("t.sv"), src).unwrap();
        let out = Command::new(xezim())
            .current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .envs(extra.iter().copied())
            .args(["--code-coverage", "t.sv"])
            .output()
            .expect("run xezim");
        assert!(out.status.success());
        let json = results(&d);
        json[json.find("\"code_coverage\"").unwrap()..].to_string()
    };
    let default = code(&[]);
    let four_state = code(&[("XEZIM_TWO_STATE", "0")]);
    assert_eq!(default, four_state);
    assert!(default.contains(&stmt(5, "always_ff", 5)), "{default}");
    let _ = std::fs::remove_dir_all(&d);
}

const SMALL: &str = "\
module sub(input logic a, output logic y);
  assign y = ~a;
  initial $display(\"sub\");
endmodule
module tb;
  int x;
  logic a = 0, y;
  sub u(.a(a), .y(y));
  initial begin
    x = 1;
    #1 if (x > 0) $display(\"x=%0d y=%b\", x, y);
    $error(\"boom\");
  end
endmodule
";

/// Without code coverage nothing changes: the output and exit status are
/// the same as with it, and no results file is written for a design with
/// no covergroup or assertion. The instance's blocks are elaborated lazily;
/// instrumenting them keeps the order they run in.
#[test]
fn off_by_default_output_unchanged() {
    let d = scratch("off");
    for extra in [&[][..], &["--error-exit"][..]] {
        let _ = std::fs::remove_file(d.join("cov.json"));
        let off = run(&d, SMALL, extra);
        assert!(!d.join("cov.json").exists());
        let mut args = extra.to_vec();
        args.push("--code-coverage");
        let on = run(&d, SMALL, &args);
        assert!(results(&d).contains("\"code_coverage\""));
        assert_eq!(off.status.code(), on.status.code());
        assert_eq!(off.stdout, on.stdout);
        assert_eq!(off.stderr, on.stderr);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Other simulators' spellings: `+cover=<letters>` picks the kinds (`c`
/// condition and `e` expression included) and warns once about the ones
/// xezim lacks; bare `+cover` and `-coverage` collect all five;
/// XEZIM_CODE_COVERAGE does what the flag does.
#[test]
fn compatible_spellings() {
    let d = scratch("compat");
    let kinds = |args: &[&str]| {
        let out = run(&d, SMALL, args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let json = results(&d);
        let start = json.find("\"kinds\": [").unwrap() + "\"kinds\": [".len();
        let k = json[start..start + json[start..].find(']').unwrap()].to_string();
        (k, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let all = "\"statement\", \"branch\", \"toggle\", \"condition\", \"expression\"";
    let (k, err) = kinds(&["+cover=sbcfx"]);
    assert_eq!(k, "\"statement\", \"branch\", \"condition\"");
    assert_eq!(
        err.matches(
            "Warning: +cover=sbcfx: xezim has no FSM (f), extended toggle (x) coverage; \
                     collecting statement, branch, condition coverage"
        )
        .count(),
        1,
        "{err}"
    );
    assert_eq!(kinds(&["+cover=t"]).0, "\"toggle\"");
    assert_eq!(kinds(&["+cover=ec"]).0, "\"condition\", \"expression\"");
    assert_eq!(kinds(&["+cover"]).0, all);
    assert_eq!(kinds(&["-coverage"]).0, all);
    assert_eq!(kinds(&["--code-coverage"]).0, all);
    assert_eq!(kinds(&["--code-coverage=all"]).0, all);
    assert_eq!(kinds(&["-coverage", "+cover=s"]).0, "\"statement\"");
    assert_eq!(
        kinds(&["--code-coverage=branch,stmt"]).0,
        "\"statement\", \"branch\""
    );
    assert_eq!(
        kinds(&["--code-coverage=expr,cond"]).0,
        "\"condition\", \"expression\""
    );
    std::fs::write(d.join("t.sv"), SMALL).unwrap();
    let env = Command::new(xezim())
        .current_dir(&d)
        .env("XEZIM_COV_DB", d.join("cov.json"))
        .env("XEZIM_CODE_COVERAGE", "toggle")
        .arg("t.sv")
        .output()
        .expect("run xezim");
    assert!(env.status.success());
    assert!(results(&d).contains("\"kinds\": [\"toggle\"]"));
    let bad = run(&d, SMALL, &["--code-coverage=lines"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown code coverage kind 'lines'"));
    let _ = std::fs::remove_dir_all(&d);
}

/// `--code-coverage-scope` keeps only the listed subtree, and `--verbose`
/// prints the totals and one line per scope.
#[test]
fn scope_limit_and_summary() {
    let d = scratch("scope");
    let out = run(
        &d,
        DESIGN,
        &[
            "--code-coverage",
            "--code-coverage-scope=tb.u0",
            "--verbose",
        ],
    );
    assert!(out.status.success());
    let json = results(&d);
    assert!(json.contains("\"scope\": \"tb.u0\""), "{json}");
    assert!(!json.contains("\"scope\": \"tb\""), "{json}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("[COV] code coverage: statement 5/5 (100.00%), branch 4/4 (100.00%), toggle"),
        "{err}"
    );
    assert!(
        err.contains("[COV]   tb.u0 (sub): statement 5/5 (100.00%)"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The input terms of the condition or expression item of kind `kind` at
/// `line`, with the counts of their two rows (the term at 0, and at 1,
/// deciding the result).
fn terms(json: &str, line: u32, kind: &str) -> Vec<(String, [u64; 2])> {
    // Statements and branches have a line and kind too; items go on with
    // their text (or generate block).
    let key = format!("\"line\": {}, \"kind\": \"{}\", ", line, kind);
    let start = json
        .match_indices(&key)
        .map(|(i, _)| i)
        .find(|&i| {
            let after = &json[i + key.len()..];
            after.starts_with("\"text\"") || after.starts_with("\"block\"")
        })
        .unwrap_or_else(|| panic!("no {kind} item at {line}: {json}"));
    let rest = &json[start..];
    let rest = &rest[rest.find("\"terms\": [").unwrap() + "\"terms\": [".len()..];
    let list = &rest[..rest.find("}]}").unwrap() + 1];
    list.split("{\"term\": \"")
        .skip(1)
        .map(|t| {
            let name = t[..t.find('"').unwrap()].to_string();
            let h = &t[t.find("\"hits\": [").unwrap() + "\"hits\": [".len()..];
            let h: Vec<u64> = h[..h.find(']').unwrap()]
                .split(", ")
                .map(|n| n.parse().unwrap())
                .collect();
            (name, [h[0], h[1]])
        })
        .collect()
}

fn has_item(json: &str, line: u32) -> bool {
    json.contains(&format!("\"line\": {}, \"kind\": \"if\", \"text\"", line))
        || ["while", "ternary", "statement", "assign", "port"]
            .iter()
            .any(|k| {
                json.contains(&format!(
                    "\"line\": {}, \"kind\": \"{}\", \"text\"",
                    line, k
                ))
            })
}

/// Condition coverage is focused expression coverage, short-circuit aware
/// (IEEE 1800-2023 §11.3.5: `&&` skips its right operand after a 0, `||`
/// after a 1): a term's row counts when the term alone decides the result.
/// The left of `&&` decides at 0 whatever the right would be; at 1 only
/// when the right is 1. Rows and terms covered are the commercial
/// simulator's on this design (4 of 6 terms; `half` never sees x or y at 1
/// deciding).
#[test]
fn condition_rows_short_circuit() {
    let d = scratch("cond");
    let src = "\
module tb;
  int n;
  task automatic check(input logic x, input logic y);
    if (x && y) n++;
    if (x || y) n++;
  endtask
  task automatic half(input logic x, input logic y);
    if (x && y) n++;
  endtask
  initial begin
    n = 0;
    check(0, 0); check(1, 0); check(1, 1); check(0, 1);
    half(0, 0); half(1, 0); half(0, 1);
    $display(\"n=%0d\", n);
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=cond"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l == "n=4")
    );
    let json = results(&d);
    assert!(json.contains("\"kinds\": [\"condition\"]"), "{json}");
    let t = |x: [u64; 2], y: [u64; 2]| vec![("x".to_string(), x), ("y".to_string(), y)];
    assert_eq!(terms(&json, 4, "if"), t([2, 1], [1, 1]));
    assert_eq!(terms(&json, 5, "if"), t([1, 2], [1, 1]));
    assert_eq!(terms(&json, 8, "if"), t([2, 0], [1, 0]));
    assert!(
        json.contains("\"condition\": {\"covered\": 4, \"total\": 6, \"percent\": 66.67}"),
        "{json}"
    );
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 4, \"kind\": \"if\", \"text\": \"x && y\", \"terms\": \
             [{\"term\": \"x\", \"hits\": [2, 1]}, {\"term\": \"y\", \"hits\": [1, 1]}]}"
        ),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Expression coverage covers the 1-bit logic values assigned, continuous
/// and procedural: `&`, `|`, `^`, `~^` on 1-bit operands decide only when
/// the other operand does not (bitwise operators evaluate both, §11.4.8),
/// a `?:` on 1-bit arms is a multiplexer whose condition always decides
/// when the other arm was not evaluated, and an item whose code never ran
/// is listed with no hits. Vector `&` and a lone reduction are not
/// expressions. The items, terms and covered terms (8 of 10) are the
/// commercial simulator's; continuous evaluations are not counted the same,
/// so only rows hit are compared there.
#[test]
fn expression_items() {
    let d = scratch("expr");
    let src = "\
module tb;
  logic a, b, c, s, x, m, n, p, q;
  logic [3:0] v, w, z;
  assign x = a & b | c;
  assign m = s ? a : b;
  assign z = v & w;
  assign p = ~^v;
  always_comb n = !(a ^ b);
  task never();
    q = a && b;
  endtask
  initial begin
    v = 4'h5; w = 4'h3;
    for (int i = 0; i < 8; i++) begin
      {a, b, c} = i[2:0];
      s = i[0];
      #1;
    end
    $display(\"x=%b m=%b n=%b z=%h p=%b\", x, m, n, z, p);
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l == "x=1 m=1 n=1 z=1 p=1")
    );
    let json = results(&d);
    assert!(
        json.contains("\"expression\": {\"covered\": 8, \"total\": 10, \"percent\": 80.00}"),
        "{json}"
    );
    let hit = |t: &[(String, [u64; 2])]| t.iter().all(|(_, h)| h[0] > 0 && h[1] > 0);
    let x = terms(&json, 4, "assign");
    assert_eq!(
        x.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert!(hit(&x), "{json}");
    let m = terms(&json, 5, "assign");
    assert_eq!(
        m.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["s", "a", "b"]
    );
    assert!(hit(&m), "{json}");
    assert!(hit(&terms(&json, 8, "statement")), "{json}");
    assert_eq!(
        terms(&json, 10, "statement"),
        vec![("a".to_string(), [0, 0]), ("b".to_string(), [0, 0])]
    );
    assert!(!has_item(&json, 6) && !has_item(&json, 7), "{json}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Which expressions are items, as in the commercial simulator's report on
/// this design: a condition or value with a logic operator on two
/// non-constant operands, or a relation; not a lone (possibly negated)
/// signal, nor a 1-bit signal compared with a constant (it is that signal),
/// nor a `for` loop's condition; a `while` condition, a `?:` condition (in
/// an assignment or a message) and an input port's connection (an
/// expression of the parent) are.
#[test]
fn which_expressions_count() {
    let d = scratch("which");
    let src = "\
module sub(input logic en, output logic q);
  assign q = en;
endmodule
module tb;
  logic a, b, c, y, y2, t;
  logic [3:0] v, w;
  logic [7:0] r;
  int i, k;
  sub u(.en(a & c), .q(t));
  initial begin
    a = 1; b = 0; c = 1; v = 3; w = 4; k = 0;
    if (a) k++;
    if (!a) k++;
    if (a == 1'b0) k++;
    if (v == 4'd3) k++;
    if (v == 3 && a) k++;
    if (!$isunknown(v) && a) k++;
    for (i = 0; i < 2; i++) k++;
    i = 0;
    while (i < 2 && a) i++;
    r = (a && b) ? 8'd1 : 8'd2;
    y = v[0] & w[1];
    y2 = ^v ^ a;
    y = r & v;
    y = (v > 2) | (w < 1);
    $display(\"%s\", (a || b) ? \"yes\" : \"no\");
    #1 $display(\"k=%0d i=%0d r=%0d y=%b y2=%b t=%b\", k, i, r, y, y2, t);
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=cond,expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    for line in [15, 16, 20, 21, 26, 9, 22, 23, 25] {
        assert!(has_item(&json, line), "line {line}: {json}");
    }
    for line in [12, 13, 14, 17, 18, 24] {
        assert!(!has_item(&json, line), "line {line}: {json}");
    }
    assert_eq!(
        terms(&json, 15, "if"),
        vec![("v == 4'd3".to_string(), [0, 1])]
    );
    assert_eq!(
        terms(&json, 9, "port"),
        vec![("a".to_string(), [0, 1]), ("c".to_string(), [0, 1])]
    );
    assert_eq!(
        terms(&json, 23, "statement"),
        vec![("^v".to_string(), [1, 0]), ("a".to_string(), [0, 1])]
    );
    assert_eq!(terms(&json, 26, "ternary").len(), 2);
    assert!(
        json.contains("\"condition\": {\"covered\": 1, \"total\": 9, \"percent\": 11.11}"),
        "{json}"
    );
    assert!(
        json.contains("\"expression\": {\"covered\": 0, \"total\": 8, \"percent\": 0.00}"),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The interpreter evaluates an item as the design would: the right of
/// `&&` and `||` only when the left does not decide (a function with a side
/// effect runs as often as without coverage, §11.3.5), and an x term gives
/// no row. The output is the same with coverage as without.
#[test]
fn items_keep_results() {
    let d = scratch("same");
    let src = "\
module tb;
  int calls = 0;
  function automatic bit f(input bit v);
    calls++;
    return v;
  endfunction
  logic a, b, y;
  initial begin
    a = 0; b = 1;
    if (a && f(b)) $display(\"taken\");
    y = a || f(b);
    if (f(a) || f(b)) $display(\"or\");
    a = 1'bx;
    if (a && b) $display(\"x\"); else $display(\"not x\");
    $display(\"calls=%0d y=%b\", calls, y);
  end
endmodule
";
    let off = run(&d, src, &[]);
    let on = run(&d, src, &["--code-coverage"]);
    assert!(on.status.success());
    assert_eq!(off.stdout, on.stdout);
    assert!(
        String::from_utf8_lossy(&on.stdout).contains("calls=3 y=1"),
        "{}",
        String::from_utf8_lossy(&on.stdout)
    );
    let json = results(&d);
    assert_eq!(
        terms(&json, 10, "if"),
        vec![("a".to_string(), [1, 0]), ("f(b)".to_string(), [0, 0])]
    );
    assert_eq!(
        terms(&json, 11, "statement"),
        vec![("a".to_string(), [0, 0]), ("f(b)".to_string(), [0, 1])]
    );
    assert_eq!(
        terms(&json, 14, "if"),
        vec![("a".to_string(), [0, 0]), ("b".to_string(), [0, 0])]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// An item of a continuous assignment is evaluated by the interpreter in the
/// assignment's instance, and leaves name resolution as it found it: the
/// testbench's `$monitor` of its own `op_select` and `start`, also the names
/// of ports of the two instances, keeps its bindings (once, it printed the
/// second instance's ports instead, `5 op_select=5 start=1`). The items of
/// the two iterations of a generate loop stay apart, as in the commercial
/// simulator.
#[test]
fn items_in_instances() {
    let d = scratch("inst");
    let src = "\
module m #(parameter int N = 4) (input logic clk, input logic [2:0] op_select,
                                 input logic start, output logic last);
  logic [3:0] row = 0;
  logic [1:0] l;
  assign last = (int'(row) == N - 1) && start;
  for (genvar i = 0; i < 2; i++) begin : g
    assign l[i] = op_select[i] | ~start;
  end
  always_ff @(posedge clk) if (start) row <= row + 1;
endmodule
module tb;
  logic clk = 0;
  logic [2:0] op_select, op_select_s;
  logic start, start_s;
  logic last, last_s;
  m #(.N(3)) dut(.clk, .op_select, .start, .last);
  m #(.N(2)) dut_small(.clk, .op_select(op_select_s), .start(start_s), .last(last_s));
  always #5 clk = ~clk;
  initial $monitor(\"%0t op_select=%0d start=%b last=%b\", $time, op_select, start, last);
  initial begin
    op_select = 0; start = 0; op_select_s = 0; start_s = 0;
    #2 start_s = 1; op_select_s = 5;
    #10 op_select = 1; start = 1;
    #20 op_select = 2;
    #20 start = 0;
    #10 $finish;
  end
endmodule
";
    let off = run(&d, src, &[]);
    let on = run(&d, src, &["--code-coverage"]);
    assert!(on.status.success());
    let out = String::from_utf8_lossy(&on.stdout);
    assert_eq!(String::from_utf8_lossy(&off.stdout), out);
    assert!(!out.contains("op_select=5"), "{out}");
    let json = results(&d);
    let rest = &json[json.find("\"scope\": \"tb.dut_small\"").unwrap() + 1..];
    let scope = &rest[..rest.find("\"design_units\"").unwrap()];
    let scope = &scope[..scope.find("\"scope\": \"").unwrap_or(scope.len())];
    assert_eq!(
        scope
            .matches("\"line\": 7, \"kind\": \"assign\", \"text\"")
            .count(),
        2,
        "{scope}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Each kind's results are the same whichever other kinds are collected: the
/// `?:` of a 1-bit value is a multiplexer of the expression item (its
/// condition no condition item) even when only conditions are collected,
/// and every `?:` is a branch either way. A `?:` of a wider value has a
/// condition item.
#[test]
fn kinds_are_independent() {
    let d = scratch("kinds");
    let src = "\
module tb;
  logic a, b, c, e, y;
  logic [3:0] w;
  initial begin
    {a, b, c, e} = 4'b1010;
    y = (a && b) ? c : e;
    w = (a || b) ? 4'd1 : 4'd2;
    #1 $display(\"y=%b w=%0d\", y, w);
  end
endmodule
";
    let total = |args: &[&str], kind: &str| {
        let out = run(&d, src, args);
        assert!(out.status.success());
        let json = results(&d);
        let key = format!("\"{}\": {{", kind);
        let at = json.find(&key).unwrap_or_else(|| panic!("{json}")) + key.len();
        json[at..at + json[at..].find('}').unwrap()].to_string()
    };
    // Line 7's `a || b` (a at 1 decides alone; b is not evaluated).
    let cond = total(&["--code-coverage=cond"], "condition");
    assert_eq!(cond, total(&["--code-coverage=cond,expr"], "condition"));
    assert!(cond.starts_with("\"covered\": 0, \"total\": 2"), "{cond}");
    let branch = total(&["--code-coverage=branch"], "branch");
    assert_eq!(
        branch,
        total(&["--code-coverage=branch,cond,expr"], "branch")
    );
    assert!(
        branch.starts_with("\"covered\": 2, \"total\": 4"),
        "{branch}"
    );
    // Line 6: a, b, c and e; b and e decide at 0.
    let expr = total(&["--code-coverage=expr"], "expression");
    assert!(expr.starts_with("\"covered\": 0, \"total\": 4"), "{expr}");
    assert_eq!(
        terms(&results(&d), 6, "statement"),
        vec![
            ("a".to_string(), [0, 0]),
            ("b".to_string(), [1, 0]),
            ("c".to_string(), [0, 0]),
            ("e".to_string(), [1, 0])
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// A part of an item that folds to a constant still has its `?:` branch; a
/// block's local declaration ends with the block (`p` is the task's 1-bit
/// argument again after it, so `p & q` is an item), and a `for` loop's
/// variable is its body's (the `int p` there makes `p & q` a vector
/// operation, no item); and the instances of a module whose parameters fold
/// different terms away keep separate items in the design unit.
#[test]
fn folded_parts_locals_and_units() {
    let d = scratch("fold");
    let src = "\
module m #(parameter bit P = 1, parameter bit Q = 0) (input logic a, b, c, d, output logic y);
  assign y = (a & b & P) | (c & d & Q);
endmodule
module tb;
  logic a, b, c, e, y, y1, y2;
  m #(.P(1), .Q(0)) u1(.a, .b, .c, .d(e), .y(y1));
  m #(.P(0), .Q(1)) u2(.a, .b, .c, .d(e), .y(y2));
  task automatic t(input logic p, input logic q);
    begin : inner
      logic [7:0] p;
      p = 8'hff;
    end
    y = p & q;
    for (int p = 0; p < 2; p++) y = p & q;
  endtask
  initial begin
    {a, b, c, e} = 4'b1010;
    y = (a & b) | ((c ? a : e) & 1'b0);
    t(a, b);
    #1 $display(\"y=%b y1=%b y2=%b\", y, y1, y2);
  end
endmodule
";
    let branch = |kinds: &str| {
        assert!(run(&d, src, &[kinds]).status.success());
        let json = results(&d);
        let at = json.find("\"branch\": {").unwrap();
        json[at..at + json[at..].find('}').unwrap()].to_string()
    };
    assert_eq!(
        branch("--code-coverage=branch"),
        branch("--code-coverage=all")
    );
    let json = results(&d);
    assert!(
        json.contains("\"branch\": {\"covered\": 1, \"total\": 2"),
        "{json}"
    );
    let names = |line: u32, kind: &str| {
        terms(&json, line, kind)
            .into_iter()
            .map(|t| t.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(13, "statement"), ["p", "q"]);
    assert!(!has_item(&json, 14), "{json}");
    assert_eq!(names(18, "statement"), ["a", "b"]);
    let unit = &json[json
        .find("\"design_unit\": \"m\",\n        \"instances\": 2")
        .unwrap()..];
    let unit = &unit[..unit.find("\"design_unit\": \"tb\"").unwrap_or(unit.len())];
    assert_eq!(terms(unit, 2, "assign").len(), 2, "{unit}");
    assert_eq!(
        unit.matches("\"line\": 2, \"kind\": \"assign\", \"text\"")
            .count(),
        2,
        "{unit}"
    );
    assert!(unit.contains("{\"term\": \"c\", "), "{unit}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Results that do not depend on the executor: the two-state executors hand
/// a block with an item to the four-state VM, which counts it once.
#[test]
fn item_counts_do_not_depend_on_executor() {
    let d = scratch("itemengine");
    let src = "\
module tb;
  logic clk = 0;
  logic [3:0] q, d;
  logic e;
  always #5 clk = ~clk;
  always_ff @(posedge clk)
    if (d[0] && !d[1]) q <= d;
    else q <= ~d;
  always_ff @(posedge clk) e <= d[2] ^ d[3];
  initial begin
    #12 d = 4'h5;
    #20 d = 4'h2;
    #20 $finish;
  end
endmodule
";
    let code = |extra: &[(&str, &str)]| {
        std::fs::write(d.join("t.sv"), src).unwrap();
        let out = Command::new(xezim())
            .current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .envs(extra.iter().copied())
            .args(["--code-coverage=cond,expr", "t.sv"])
            .output()
            .expect("run xezim");
        assert!(out.status.success());
        let json = results(&d);
        json[json.find("\"code_coverage\"").unwrap()..].to_string()
    };
    let default = code(&[]);
    let four_state = code(&[("XEZIM_TWO_STATE", "0")]);
    assert_eq!(default, four_state);
    // d is x at the first edge (no row), 5 at two, 2 at the last two.
    assert_eq!(
        terms(&default, 7, "if"),
        vec![("d[0]".to_string(), [2, 2]), ("d[1]".to_string(), [2, 0])]
    );
    let _ = std::fs::remove_dir_all(&d);
}
