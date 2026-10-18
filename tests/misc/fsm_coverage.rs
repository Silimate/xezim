//! FSM coverage (`--code-coverage=fsm`, docs/coverage-guide.md, "FSMs"):
//! which variables are state machines, their states and transitions, and
//! the counts. IEEE 1800 does not define FSM coverage; each expected value
//! below is the reference simulator's report on the same design and
//! stimulus (its FSM states with values, lines and visit counts, and its
//! transitions with lines and counts). The designs use enums (§6.19),
//! parameters and `define constants (§6.20, §22.5), `case` (§12.5), `if`
//! (§12.4) and `always_ff` / `always_comb` (§9.2.2).
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
    let d = std::env::temp_dir().join(format!("xezim_fsm_cov_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run xezim on `src` (written to `t.sv` in `dir`) with `args`; the results
/// file is `cov.json` there.
fn run(dir: &Path, src: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
    std::fs::write(dir.join("t.sv"), src).unwrap();
    Command::new(xezim())
        .current_dir(dir)
        .env("XEZIM_COV_DB", dir.join("cov.json"))
        .env_remove("XEZIM_CODE_COVERAGE")
        .env_remove("XEZIM_VERBOSE")
        .envs(env.iter().copied())
        .args(args)
        .arg("t.sv")
        .output()
        .expect("run xezim")
}

fn results(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("cov.json")).unwrap_or_default()
}

/// `"key": value` in a flat JSON object, quotes dropped.
fn get<'a>(obj: &'a str, key: &str) -> &'a str {
    let k = format!("\"{}\": ", key);
    let rest = &obj[obj.find(&k).unwrap_or_else(|| panic!("no {key} in {obj}")) + k.len()..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().trim_matches('"')
}

/// The objects of a flat JSON array's text.
fn objects(list: &str) -> Vec<&str> {
    let list = list.trim().trim_start_matches('{').trim_end_matches('}');
    if list.is_empty() {
        return Vec::new();
    }
    list.split("}, {").collect()
}

/// One FSM as the tests write it: scope, variable, then each state as
/// `NAME=VALUE@LINE:COUNT` and each transition as `FROM>TO@LINE:COUNT`.
type Expect = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

type Found = (String, String, Vec<String>, Vec<String>);

/// Every FSM of the scopes in the results file, in the form of `Expect`.
fn fsms(json: &str) -> Vec<Found> {
    let mut out = Vec::new();
    let mut scope = String::new();
    for line in json.lines().map(str::trim) {
        if line.starts_with("\"design_units\"") {
            break;
        }
        if line.starts_with("\"scope\": ") {
            scope = get(line, "scope").to_string();
        }
        let Some(list) = line.strip_prefix("\"fsms\": [") else {
            continue;
        };
        for f in list.split("{\"variable\": ").skip(1) {
            let var = f[1..f[1..].find('"').unwrap() + 1].to_string();
            let states =
                &f[f.find("\"states\": [").unwrap() + 11..f.find("], \"transitions\"").unwrap()];
            let trans = &f
                [f.find("\"transitions\": [").unwrap() + 16..f.find("], \"unreachable\"").unwrap()];
            out.push((
                scope.clone(),
                var,
                objects(states)
                    .iter()
                    .map(|s| {
                        format!(
                            "{}={}@{}:{}",
                            get(s, "name"),
                            get(s, "value"),
                            get(s, "line"),
                            get(s, "count")
                        )
                    })
                    .collect(),
                objects(trans)
                    .iter()
                    .map(|t| {
                        format!(
                            "{}>{}@{}:{}",
                            get(t, "from"),
                            get(t, "to"),
                            get(t, "line"),
                            get(t, "count")
                        )
                    })
                    .collect(),
            ));
        }
    }
    out.sort();
    out
}

/// Run `src` with FSM coverage and check its FSMs, all of them, against the
/// reference simulator's.
fn check(tag: &str, src: &str, want: &[Expect]) -> String {
    check_lines(tag, src, want, true)
}

/// `check`, optionally leaving out the states' lines.
fn check_lines(tag: &str, src: &str, want: &[Expect], state_lines: bool) -> String {
    let d = scratch(tag);
    let out = run(&d, src, &["--code-coverage=fsm"], &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let mut want: Vec<Found> = want
        .iter()
        .map(|(sc, v, s, t)| {
            (
                sc.to_string(),
                v.to_string(),
                s.iter().map(|x| x.to_string()).collect(),
                t.iter().map(|x| x.to_string()).collect(),
            )
        })
        .collect();
    want.sort();
    let mut got = fsms(&json);
    if !state_lines {
        let strip = |v: &mut Vec<String>| {
            for x in v.iter_mut() {
                let (a, b) = x.split_once('@').unwrap();
                let (_, c) = b.split_once(':').unwrap();
                *x = format!("{a}:{c}");
            }
        };
        for f in got.iter_mut().chain(want.iter_mut()) {
            strip(&mut f.2);
        }
    }
    assert_eq!(got, want, "{json}");
    let _ = std::fs::remove_dir_all(&d);
    json
}

const DOOR: &str = r#"module door (input logic clk, rst, open_req, close_req, output logic busy);
  typedef enum logic [1:0] {CLOSED, OPENING, OPEN, CLOSING} state_t;
  state_t state, next;
  always_ff @(posedge clk)
    if (rst) state <= CLOSED;
    else     state <= next;
  always_comb begin
    next = state;
    case (state)
      CLOSED:  if (open_req)  next = OPENING;
      OPENING: next = OPEN;
      OPEN:    if (close_req) next = CLOSING;
      CLOSING: if (open_req)  next = OPENING;
               else           next = CLOSED;
    endcase
  end
  assign busy = state == OPENING || state == CLOSING;
endmodule

module tb;
  logic clk = 0, rst = 1, open_req = 0, close_req = 0, busy;
  door u_door (.*);
  always #5 clk = ~clk;
  initial begin
    @(negedge clk) rst = 0;
    open_req = 1;  @(negedge clk) open_req = 0;
    repeat (3) @(negedge clk);
    close_req = 1; @(negedge clk) close_req = 0;
    repeat (2) @(negedge clk);
    $finish;
  end
endmodule
"#;

const DOOR_FSMS: &[Expect] = &[(
    "tb.u_door",
    "state",
    &[
        "CLOSED=0@10:3",
        "OPENING=1@11:1",
        "OPEN=2@12:2",
        "CLOSING=3@13:1",
    ],
    &[
        "CLOSED>OPENING@10:1",
        "OPENING>CLOSED@5:0",
        "OPENING>OPEN@11:1",
        "OPEN>CLOSED@5:0",
        "OPEN>CLOSING@12:1",
        "CLOSING>CLOSED@14:1",
        "CLOSING>OPENING@13:0",
    ],
)];

const DEFAULT_ARM: &str = r#"// Two-process enum FSM: sync reset, default hold, explicit self-transition,
// default arm handling S3, unreachable S4, never-mentioned S5.
module e1(input logic clk, rst, a, b);
  typedef enum logic [2:0] {S0, S1, S2, S3, S4, S5} st_t;
  st_t cs, ns;
  always_ff @(posedge clk) if (rst) cs <= S0; else cs <= ns;
  always_comb begin
    ns = cs;
    case (cs)
      S0: if (a) ns = S1; else ns = S0;
      S1: if (b) ns = S2;
      S2: ns = S3;
      S4: ns = S0;
      default: ns = S0;
    endcase
  end
endmodule
module tb;
  logic clk = 0, rst = 1, a = 0, b = 0;
  e1 u(.*);
  always #5 clk = ~clk;
  initial begin
    repeat (3) @(negedge clk);
    rst = 0;
    repeat (2) @(negedge clk);
    a = 1; @(negedge clk); a = 0;
    repeat (3) @(negedge clk);
    b = 1; @(negedge clk); b = 0;
    repeat (4) @(negedge clk);
    a = 1; @(negedge clk); a = 0;
    rst = 1; @(negedge clk); rst = 0;
    repeat (2) @(negedge clk);
    $finish;
  end
endmodule
"#;

const DEFAULT_ARM_FSMS: &[Expect] = &[(
    "tb.u",
    "cs",
    &[
        "S0=0@10:7",
        "S1=1@11:3",
        "S2=2@12:1",
        "S3=3@14:1",
        "S4=4@13:0",
    ],
    &[
        "S0>S1@10:2",
        "S1>S0@6:1",
        "S1>S2@11:1",
        "S2>S0@6:0",
        "S2>S3@12:1",
        "S3>S0@14:1",
        "S4>S0@13:0",
    ],
)];

const ONE_PROCESS: &str = r#"// One-process localparam FSM, async reset, enable hold, double write in one step.
module e2(input logic clk, rst_n, en, x);
  localparam [1:0] A = 2'd0, B = 2'd1, C = 2'd2;
  reg [1:0] st;
  always @(posedge clk or negedge rst_n)
    if (!rst_n) st <= A;
    else if (en)
      case (st)
        A: st <= B;
        B: begin st <= A; if (x) st <= C; end
        C: st <= A;
      endcase
endmodule
module tb;
  logic clk = 0, rst_n = 0, en = 0, x = 0;
  e2 u(.*);
  always #5 clk = ~clk;
  initial begin
    repeat (2) @(negedge clk);
    rst_n = 1;
    repeat (3) @(negedge clk);
    en = 1; @(negedge clk); en = 0;      // A->B
    repeat (3) @(negedge clk);
    en = 1; x = 1; @(negedge clk); en = 0; x = 0;   // B->C (double write)
    repeat (2) @(negedge clk);
    en = 1; @(negedge clk);              // C->A
    @(negedge clk);                       // A->B
    @(negedge clk);                       // B->A
    en = 0;
    #2 rst_n = 0; #1 rst_n = 1;           // async reset pulse in A
    repeat (2) @(negedge clk);
    $finish;
  end
endmodule
"#;

const ONE_PROCESS_FSMS: &[Expect] = &[(
    "tb.u",
    "st",
    &["A=0@9:5", "B=1@10:2", "C=2@11:1"],
    &["A>B@9:2", "B>A@10:1", "B>C@10:1", "C>A@11:1"],
)];

const IF_CHAIN: &str = r#"// `define encoding, next state from an if/else-if chain (no case).
`define I 2'b00
`define R 2'b01
`define D 2'b10
module e3(input logic clk, rst, go);
  logic [1:0] st, ns;
  always_ff @(posedge clk or posedge rst) if (rst) st <= `I; else st <= ns;
  always @* begin
    ns = st;
    if (st == `I) begin if (go) ns = `R; end
    else if (st == `R) ns = `D;
    else if (st == `D) ns = `I;
  end
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0;
  e3 u(.*);
  always #5 clk = ~clk;
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    go = 1; @(negedge clk); go = 0;
    repeat (4) @(negedge clk);
    $finish;
  end
endmodule
"#;

const IF_CHAIN_FSMS: &[Expect] = &[(
    "tb.u",
    "st",
    &["st0=0@10:4", "st1=1@11:1", "st2=2@12:1"],
    &[
        "st0>st1@10:1",
        "st1>st0@7:0",
        "st1>st2@11:1",
        "st2>st0@12:1",
    ],
)];

const TERNARY: &str = r#"// A ternary next state, a counter under a case on itself, a 1-bit state.
module e4(input logic clk, rst, go, sel);
  localparam [1:0] P = 0, Q = 1, R = 2;
  logic [1:0] t;    // ternary assignment
  always_ff @(posedge clk)
    if (rst) t <= P;
    else case (t)
      P: if (go) t <= Q;
      Q: t <= sel ? R : P;
      R: t <= P;
    endcase
  logic [1:0] c;    // counter
  always_ff @(posedge clk)
    if (rst) c <= 0;
    else case (c)
      2'd0: if (go) c <= c + 1;
      2'd1: c <= c + 1;
      default: c <= 0;
    endcase
  logic b1;         // single bit
  always_ff @(posedge clk)
    if (rst) b1 <= 1'b0;
    else case (b1)
      1'b0: if (go) b1 <= 1'b1;
      1'b1: b1 <= 1'b0;
    endcase
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0, sel = 0;
  e4 u(.*);
  always #5 clk = ~clk;
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    go = 1; @(negedge clk); go = 0;
    repeat (4) @(negedge clk);
    $finish;
  end
endmodule
"#;

const TERNARY_FSMS: &[Expect] = &[
    (
        "tb.u",
        "c",
        &["st0=0@16:3", "st1=1@17:1", "st2=2@18:1"],
        &[
            "st0>st1@16:1",
            "st1>st0@14:0",
            "st1>st2@17:1",
            "st2>st0@18:1",
        ],
    ),
    (
        "tb.u",
        "t",
        &["P=0@8:3", "Q=1@9:1", "R=2@10:0"],
        &["P>Q@8:1", "Q>P@9:1", "Q>R@9:0", "R>P@10:0"],
    ),
];

const PARAMS: &str = r#"// Parameter encoding, one-process, non-reset clear outside the case, two FSMs,
// no reset on the second, explicit self transition one-process.
module e5 #(parameter W = 2)(input logic clk, rst, go, clr);
  parameter [W-1:0] I = 0, R = 1, D = 2, E = 3;
  reg [W-1:0] st;
  always @(posedge clk) begin
    if (rst) st <= I;
    else begin
      case (st)
        I: if (go) st <= R; else st <= I;
        R: st <= D;
        D: st <= E;
        E: st <= I;
      endcase
      if (clr) st <= I;
    end
  end
  typedef enum {X0, X1, X2} xs_t;
  xs_t xs;
  initial xs = X0;
  always @(posedge clk)
    case (xs)
      X0: if (go) xs <= X1;
      X1: xs <= X2;
      X2: xs <= X0;
    endcase
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0, clr = 0;
  e5 u(.*);
  always #5 clk = ~clk;
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    go = 1; @(negedge clk); go = 0;
    @(negedge clk);
    clr = 1; @(negedge clk); clr = 0;
    go = 1; @(negedge clk); go = 0;
    repeat (5) @(negedge clk);
    $finish;
  end
endmodule
"#;

const PARAMS_FSMS: &[Expect] = &[
    (
        "tb.u",
        "st",
        &["I=0@10:5", "R=1@11:2", "D=2@12:2", "E=3@13:1"],
        &[
            "I>R@10:2", "R>I@15:0", "R>D@11:2", "D>I@15:1", "D>E@12:1", "E>I@13:1",
        ],
    ),
    (
        "tb.u",
        "xs",
        &["X0=0@23:2", "X1=1@24:2", "X2=2@25:2"],
        &["X0>X1@23:2", "X1>X2@24:2", "X2>X0@25:2"],
    ),
];

const VISITS: &str = r#"// Two-process FSM, several instances with distinct stimuli, to pin down hit counting.
module f2(input logic clk, rst, a, b, c);
  typedef enum logic [1:0] {A, B, C, D} st_t;
  st_t cs, ns;
  always_ff @(posedge clk) if (rst) cs <= A; else cs <= ns;
  always_comb begin
    ns = cs;
    case (cs)
      A: if (a) ns = B;
      B: if (b) ns = C;
      C: ns = D;
      D: if (c) ns = A;
    endcase
  end
endmodule
// One-process, no hold write.
module f1(input logic clk, rst, a, b, c);
  typedef enum logic [1:0] {A, B, C, D} st_t;
  st_t st;
  always_ff @(posedge clk)
    if (rst) st <= A;
    else case (st)
      A: if (a) st <= B;
      B: if (b) st <= C;
      C: st <= D;
      D: if (c) st <= A;
    endcase
endmodule
// One-process, explicit hold write every cycle.
module fh(input logic clk, rst, a, b, c);
  typedef enum logic [1:0] {A, B, C, D} st_t;
  st_t st;
  always_ff @(posedge clk)
    if (rst) st <= A;
    else begin
      st <= st;
      case (st)
        A: if (a) st <= B;
        B: if (b) st <= C;
        C: st <= D;
        D: if (c) st <= A;
      endcase
    end
endmodule
module tb;
  logic clk = 0;
  always #5 clk = ~clk;
  // Per stimulus k: rst, a, b, c vectors.
  logic [5:0] rst = '1, a = 0, b = 0, c = 0;
  for (genvar k = 0; k < 6; k++) begin : g
    f2 u2(.clk, .rst(rst[k]), .a(a[k]), .b(b[k]), .c(c[k]));
    f1 u1(.clk, .rst(rst[k]), .a(a[k]), .b(b[k]), .c(c[k]));
    fh uh(.clk, .rst(rst[k]), .a(a[k]), .b(b[k]), .c(c[k]));
  end
  int cyc = 0;
  always @(negedge clk) begin
    cyc <= cyc + 1;
    // k0: reset one cycle, then idle.
    rst[0] <= cyc < 1;
    // k1: reset one cycle, then c toggles every cycle (irrelevant input).
    rst[1] <= cyc < 1; c[1] <= cyc[0];
    // k2: reset four cycles, then idle.
    rst[2] <= cyc < 4;
    // k3: reset, A->B at cycle 3, then stay in B.
    rst[3] <= cyc < 1; a[3] <= cyc == 3;
    // k4: reset, A->B at 3, B->C at 7, C->D, D held, D->A at 15, reset again at 18.
    rst[4] <= cyc < 1 || cyc == 18; a[4] <= cyc == 3; b[4] <= cyc == 7; c[4] <= cyc == 15;
    // k5: no reset; a toggles.
    rst[5] <= 0; a[5] <= cyc[0];
    if (cyc == 20) $finish;
  end
endmodule
"#;

const VISITS_FSMS: &[Expect] = &[
    (
        "tb.g[0].u1",
        "st",
        &["A=0@23:2", "B=1@24:0", "C=2@25:0", "D=3@26:0"],
        &[
            "A>B@23:0", "B>A@21:0", "B>C@24:0", "C>A@21:0", "C>D@25:0", "D>A@26:0",
        ],
    ),
    (
        "tb.g[0].u2",
        "cs",
        &["A=0@9:3", "B=1@10:0", "C=2@11:0", "D=3@12:0"],
        &[
            "A>B@9:0", "B>A@5:0", "B>C@10:0", "C>A@5:0", "C>D@11:0", "D>A@12:0",
        ],
    ),
    (
        "tb.g[0].uh",
        "st",
        &["A=0@38:3", "B=1@39:0", "C=2@40:0", "D=3@41:0"],
        &[
            "A>B@38:0", "B>A@34:0", "B>C@39:0", "C>A@34:0", "C>D@40:0", "D>A@41:0",
        ],
    ),
    (
        "tb.g[1].u1",
        "st",
        &["A=0@23:2", "B=1@24:0", "C=2@25:0", "D=3@26:0"],
        &[
            "A>B@23:0", "B>A@21:0", "B>C@24:0", "C>A@21:0", "C>D@25:0", "D>A@26:0",
        ],
    ),
    (
        "tb.g[1].u2",
        "cs",
        &["A=0@9:3", "B=1@10:0", "C=2@11:0", "D=3@12:0"],
        &[
            "A>B@9:0", "B>A@5:0", "B>C@10:0", "C>A@5:0", "C>D@11:0", "D>A@12:0",
        ],
    ),
    (
        "tb.g[1].uh",
        "st",
        &["A=0@38:21", "B=1@39:0", "C=2@40:0", "D=3@41:0"],
        &[
            "A>B@38:0", "B>A@34:0", "B>C@39:0", "C>A@34:0", "C>D@40:0", "D>A@41:0",
        ],
    ),
    (
        "tb.g[2].u1",
        "st",
        &["A=0@23:2", "B=1@24:0", "C=2@25:0", "D=3@26:0"],
        &[
            "A>B@23:0", "B>A@21:0", "B>C@24:0", "C>A@21:0", "C>D@25:0", "D>A@26:0",
        ],
    ),
    (
        "tb.g[2].u2",
        "cs",
        &["A=0@9:3", "B=1@10:0", "C=2@11:0", "D=3@12:0"],
        &[
            "A>B@9:0", "B>A@5:0", "B>C@10:0", "C>A@5:0", "C>D@11:0", "D>A@12:0",
        ],
    ),
    (
        "tb.g[2].uh",
        "st",
        &["A=0@38:3", "B=1@39:0", "C=2@40:0", "D=3@41:0"],
        &[
            "A>B@38:0", "B>A@34:0", "B>C@39:0", "C>A@34:0", "C>D@40:0", "D>A@41:0",
        ],
    ),
    (
        "tb.g[3].u1",
        "st",
        &["A=0@23:2", "B=1@24:1", "C=2@25:0", "D=3@26:0"],
        &[
            "A>B@23:1", "B>A@21:0", "B>C@24:0", "C>A@21:0", "C>D@25:0", "D>A@26:0",
        ],
    ),
    (
        "tb.g[3].u2",
        "cs",
        &["A=0@9:3", "B=1@10:2", "C=2@11:0", "D=3@12:0"],
        &[
            "A>B@9:1", "B>A@5:0", "B>C@10:0", "C>A@5:0", "C>D@11:0", "D>A@12:0",
        ],
    ),
    (
        "tb.g[3].uh",
        "st",
        &["A=0@38:3", "B=1@39:2", "C=2@40:0", "D=3@41:0"],
        &[
            "A>B@38:1", "B>A@34:0", "B>C@39:0", "C>A@34:0", "C>D@40:0", "D>A@41:0",
        ],
    ),
    (
        "tb.g[4].u1",
        "st",
        &["A=0@23:4", "B=1@24:1", "C=2@25:1", "D=3@26:1"],
        &[
            "A>B@23:1", "B>A@21:0", "B>C@24:1", "C>A@21:0", "C>D@25:1", "D>A@26:1",
        ],
    ),
    (
        "tb.g[4].u2",
        "cs",
        &["A=0@9:7", "B=1@10:2", "C=2@11:1", "D=3@12:2"],
        &[
            "A>B@9:1", "B>A@5:0", "B>C@10:1", "C>A@5:0", "C>D@11:1", "D>A@12:1",
        ],
    ),
    (
        "tb.g[4].uh",
        "st",
        &["A=0@38:7", "B=1@39:2", "C=2@40:1", "D=3@41:2"],
        &[
            "A>B@38:1", "B>A@34:0", "B>C@39:1", "C>A@34:0", "C>D@40:1", "D>A@41:1",
        ],
    ),
    (
        "tb.g[5].u1",
        "st",
        &["A=0@23:1", "B=1@24:1", "C=2@25:0", "D=3@26:0"],
        &[
            "A>B@23:1", "B>A@21:0", "B>C@24:0", "C>A@21:0", "C>D@25:0", "D>A@26:0",
        ],
    ),
    (
        "tb.g[5].u2",
        "cs",
        &["A=0@9:2", "B=1@10:2", "C=2@11:0", "D=3@12:0"],
        &[
            "A>B@9:1", "B>A@5:0", "B>C@10:0", "C>A@5:0", "C>D@11:0", "D>A@12:0",
        ],
    ),
    (
        "tb.g[5].uh",
        "st",
        &["A=0@38:2", "B=1@39:19", "C=2@40:0", "D=3@41:0"],
        &[
            "A>B@38:1", "B>A@34:0", "B>C@39:0", "C>A@34:0", "C>D@40:0", "D>A@41:0",
        ],
    ),
];

const STYLES: &str = r#"package p7;
  typedef enum logic [1:0] {PA, PB, PC} pst_t;
endpackage
module m_a(input logic clk, rst, go, stop);   // constant-only register, no case on itself
  logic [1:0] mode;
  always_ff @(posedge clk)
    if (rst) mode <= 2'd0;
    else if (go) mode <= 2'd1;
    else if (stop) mode <= 2'd2;
endmodule
module m_b(input logic clk, rst, go);         // cast of a constant
  typedef enum logic [1:0] {A, B, C} st_t;
  st_t st;
  always_ff @(posedge clk)
    if (rst) st <= A;
    else case (st)
      A: if (go) st <= st_t'(B);
      B: st <= C;
      C: st <= A;
    endcase
endmodule
module m_c(input logic clk, rst, go, sel);    // cast of a ternary
  typedef enum logic [1:0] {A, B, C} st_t;
  st_t st;
  always_ff @(posedge clk)
    if (rst) st <= A;
    else case (st)
      A: if (go) st <= st_t'(sel ? B : C);
      B: st <= C;
      C: st <= A;
    endcase
endmodule
module m_d(input logic clk, rst, go);         // two states in two bits
  logic [1:0] st;
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: st <= 2'd0;
    endcase
endmodule
module m_e(input logic clk, rst, go);         // unique case, multi-label item, casez
  typedef enum logic [2:0] {A, B, C, D} st_t;
  st_t cs, ns;
  always_ff @(posedge clk) if (rst) cs <= A; else cs <= ns;
  always_comb begin
    ns = cs;
    unique case (cs)
      A: if (go) ns = B;
      B, C: ns = D;
      D: ns = A;
    endcase
  end
endmodule
module m_f(input logic clk, rst, go);         // next state from a continuous assign
  localparam [1:0] A = 0, B = 1, C = 2;
  logic [1:0] cs, ns;
  always_ff @(posedge clk) if (rst) cs <= A; else cs <= ns;
  assign ns = (cs == A) ? (go ? B : A) : (cs == B) ? C : A;
endmodule
module m_g(input logic clk, rst, input logic [1:0] in);   // state from input
  localparam [1:0] A = 0, B = 1, C = 2;
  logic [1:0] st;
  always_ff @(posedge clk)
    if (rst) st <= A;
    else case (st)
      A: st <= in;
      B: st <= C;
      C: st <= A;
    endcase
endmodule
module m_h(input logic clk, rst, go);         // default next = IDLE, not hold
  typedef enum logic [1:0] {I, R, D} st_t;
  st_t cs, ns;
  always_ff @(posedge clk or posedge rst) if (rst) cs <= I; else cs <= ns;
  always @(cs or go) begin
    ns = I;
    case (cs)
      I: if (go) ns = R;
      R: ns = D;
      D: if (go) ns = D;
    endcase
  end
endmodule
module m_i(input logic clk, rst, go);         // package enum, int state var with if chain
  p7::pst_t st;
  always_ff @(posedge clk)
    if (rst) st <= p7::PA;
    else if (st == p7::PA) begin if (go) st <= p7::PB; end
    else if (st == p7::PB) st <= p7::PC;
    else st <= p7::PA;
endmodule
module m_j(input logic clk, rst, go);         // integer state var, localparams
  localparam integer A = 0, B = 1, C = 2;
  integer st;
  always @(posedge clk)
    if (rst) st <= A;
    else case (st)
      A: if (go) st <= B;
      B: st <= C;
      C: st <= A;
    endcase
endmodule
module m_k(input logic clk, rst, go);         // FSM in a generate block
  for (genvar g = 0; g < 2; g++) begin : gen
    logic [1:0] st;
    always_ff @(posedge clk)
      if (rst) st <= 0;
      else case (st)
        0: if (go) st <= 1;
        1: st <= 2;
        2: st <= 0;
      endcase
  end
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0, stop = 0, sel = 0;
  logic [1:0] in = 0;
  always #5 clk = ~clk;
  m_a ua(.*); m_b ub(.*); m_c uc(.*); m_d ud(.*); m_e ue(.*); m_f uf(.*);
  m_g ug(.*); m_h uh(.*); m_i ui(.*); m_j uj(.*); m_k uk(.*);
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    repeat (3) begin
      go = 1; @(negedge clk); go = 0; sel = ~sel; in = in + 1;
      repeat (3) @(negedge clk);
    end
    stop = 1; @(negedge clk); stop = 0;
    $finish;
  end
endmodule
"#;

const STYLES_FSMS: &[Expect] = &[
    (
        "tb.ub",
        "st",
        &["A=0@17:5", "B=1@18:3", "C=2@19:3"],
        &["A>B@17:3", "B>A@15:0", "B>C@18:3", "C>A@19:3"],
    ),
    (
        "tb.ud",
        "st",
        &["st0=0@38:5", "st1=1@39:3"],
        &["st0>st1@38:3", "st1>st0@39:3"],
    ),
    (
        "tb.ue",
        "cs",
        &["A=0@49:8", "B=1@50:3", "C=2@50:0", "D=3@51:3"],
        &[
            "A>B@49:3", "B>A@45:0", "B>D@50:3", "C>A@45:0", "C>D@50:0", "D>A@51:3",
        ],
    ),
    (
        "tb.uf",
        "cs",
        &["A=0@59:8", "B=1@59:3", "C=2@59:3"],
        &["A>B@59:3", "B>A@58:0", "B>C@59:3", "C>A@59:3"],
    ),
    (
        "tb.uh",
        "cs",
        &["I=0@79:8", "R=1@80:3", "D=2@75:3"],
        &["I>R@79:3", "R>I@75:0", "R>D@80:3", "D>I@75:3"],
    ),
    (
        "tb.ui",
        "st",
        &["PA=0@89:5", "PB=1@90:3", "PC=2@91:3"],
        &["PA>PB@89:3", "PB>PA@88:0", "PB>PC@90:3", "PC>PA@91:3"],
    ),
    (
        "tb.uj",
        "st",
        &["A=0@99:5", "B=1@100:3", "C=2@101:3"],
        &["A>B@99:3", "B>A@97:0", "B>C@100:3", "C>A@101:3"],
    ),
    (
        "tb.uk",
        "gen[0].st",
        &["st0=0@110:5", "st1=1@111:3", "st2=2@112:3"],
        &[
            "st0>st1@110:3",
            "st1>st0@108:0",
            "st1>st2@111:3",
            "st2>st0@112:3",
        ],
    ),
    (
        "tb.uk",
        "gen[1].st",
        &["st0=0@110:5", "st1=1@111:3", "st2=2@112:3"],
        &[
            "st0>st1@110:3",
            "st1>st0@108:0",
            "st1>st2@111:3",
            "st2>st0@112:3",
        ],
    ),
];

const PREVIOUS: &str = r#"// Where does the previous state come from? Writes outside the FSM process.
module m_init(input logic clk, go);           // declaration initializer
  typedef enum logic [1:0] {A, B, C} st_t;
  st_t st = B;
  always @(posedge clk)
    case (st)
      A: if (go) st <= B;
      B: if (go) st <= C;
      C: if (go) st <= A;
    endcase
endmodule
module m_late(input logic clk, go);           // initial block writes at time 3
  typedef enum logic [1:0] {A, B, C} st_t;
  st_t st;
  initial begin st = A; #3 st = C; end
  always @(posedge clk)
    case (st)
      A: if (go) st <= B;
      B: if (go) st <= C;
      C: if (go) st <= A;
    endcase
endmodule
module m_tbw(input logic clk, go);            // testbench writes the state later
  typedef enum logic [1:0] {A, B, C} st_t;
  st_t st;
  always @(posedge clk)
    case (st)
      A: if (go) st <= B;
      B: if (go) st <= C;
      C: if (go) st <= A;
      default: st <= A;
    endcase
endmodule
module tb;
  logic clk = 0, go = 0;
  always #5 clk = ~clk;
  m_init ui(.*); m_late ul(.*); m_tbw ut(.*);
  initial begin
    repeat (2) @(negedge clk);
    go = 1; @(negedge clk); go = 0;          // posedge 25: one step
    repeat (2) @(negedge clk);
    #1 ut.st = ut.C;                          // t=51: external write
    repeat (2) @(negedge clk);
    go = 1; @(negedge clk); go = 0;          // posedge 85: one step
    repeat (2) @(negedge clk);
    $finish;
  end
endmodule
"#;

const PREVIOUS_FSMS: &[Expect] = &[
    (
        "tb.ui",
        "st",
        &["A=0@7:1", "B=1@8:0", "C=2@9:1"],
        &["A>B@7:0", "B>C@8:1", "C>A@9:1"],
    ),
    (
        "tb.ul",
        "st",
        &["A=0@18:1", "B=1@19:1", "C=2@20:0"],
        &["A>B@18:1", "B>C@19:0", "C>A@20:0"],
    ),
    (
        "tb.ut",
        "st",
        &["A=0@28:2", "B=1@29:1", "C=2@30:0"],
        &["A>B@28:1", "B>C@29:0", "C>A@30:0"],
    ),
];

const ASYNC: &str = r#"// An asynchronous reset that a flop drives: it rises in the same time slot
// as the clock edge, after the state process has run for the edge.
module f2(input logic clk, rst, a);
  typedef enum logic [1:0] {A, B, C} st_t;
  st_t cs, ns;
  always_ff @(posedge clk or posedge rst) if (rst) cs <= A; else cs <= ns;
  always_comb begin
    ns = cs;
    case (cs)
      A: if (a) ns = B;
      B: ns = C;
      C: if (!a) ns = A;
    endcase
  end
endmodule
module f1(input logic clk, rst, a);
  typedef enum logic [1:0] {A, B, C} st_t;
  st_t st;
  always @(posedge clk or posedge rst)
    if (rst) st <= A;
    else case (st)
      A: if (a) st <= B;
      B: st <= C;
      C: if (!a) st <= A;
    endcase
endmodule
module tb;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [2:0] req = '1, rst = '0, a = 0;
  always @(posedge clk) rst <= req;
  for (genvar k = 0; k < 3; k++) begin : g
    f2 u2(.clk, .rst(rst[k]), .a(a[k]));
    f1 u1(.clk, .rst(rst[k]), .a(a[k]));
  end
  int cyc = 0;
  always @(negedge clk) begin
    cyc <= cyc + 1;
    // lane 0: reset 2 cycles, idle, reset again for 3 cycles while in A
    req[0] <= cyc < 2 || (cyc >= 6 && cyc < 9);
    // lane 1: reset, move to C and hold, reset while in C
    req[1] <= cyc < 2 || cyc == 7;  a[1] <= cyc >= 3;
    // lane 2: reset, move through B to C, back to A, reset in A
    req[2] <= cyc < 2 || cyc == 9;  a[2] <= cyc == 3;
    if (cyc == 14) $finish;
  end
endmodule
"#;

const ASYNC_FSMS: &[Expect] = &[
    (
        "tb.g[0].u1",
        "st",
        &["A=0@22:4", "B=1@23:0", "C=2@24:0"],
        &["A>B@22:0", "B>A@20:0", "B>C@23:0", "C>A@24:0"],
    ),
    (
        "tb.g[0].u2",
        "cs",
        &["A=0@10:6", "B=1@11:0", "C=2@12:0"],
        &["A>B@10:0", "B>A@6:0", "B>C@11:0", "C>A@12:0"],
    ),
    (
        "tb.g[1].u1",
        "st",
        &["A=0@22:4", "B=1@23:2", "C=2@24:2"],
        &["A>B@22:2", "B>A@20:0", "B>C@23:2", "C>A@24:1"],
    ),
    (
        "tb.g[1].u2",
        "cs",
        &["A=0@10:4", "B=1@11:2", "C=2@12:4"],
        &["A>B@10:2", "B>A@6:0", "B>C@11:2", "C>A@12:1"],
    ),
    (
        "tb.g[2].u1",
        "st",
        &["A=0@22:5", "B=1@23:1", "C=2@24:1"],
        &["A>B@22:1", "B>A@20:0", "B>C@23:1", "C>A@24:1"],
    ),
    (
        "tb.g[2].u2",
        "cs",
        &["A=0@10:7", "B=1@11:1", "C=2@12:1"],
        &["A>B@10:1", "B>A@6:0", "B>C@11:1", "C>A@12:1"],
    ),
];

const FILL: &str = r#"// Unbased unsized literals as states, and a parameterized FSM whose
// instances have two different state sets.
module fill(input logic clk, rst, go);
  logic [1:0] st;
  always_ff @(posedge clk)
    if (rst) st <= '0;
    else case (st)
      '0: if (go) st <= 2'd1;
      2'd1: st <= '1;
      '1: st <= '0;
    endcase
endmodule
module shaped #(parameter logic [1:0] LAST = 2'd2) (input logic clk, rst, go);
  logic [1:0] st;
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: st <= LAST;
      LAST: st <= 2'd0;
    endcase
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0;
  always #5 clk = ~clk;
  fill uf(.*);
  shaped #(.LAST(2'd2)) ua(.*);
  shaped #(.LAST(2'd3)) ub(.*);
  shaped #(.LAST(2'd3)) uc(.clk, .rst, .go(1'b1));
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    repeat (2) begin
      go = 1; @(negedge clk); go = 0;
      repeat (3) @(negedge clk);
    end
    $finish;
  end
endmodule
"#;

const FILL_FSMS: &[Expect] = &[
    (
        "tb.ua",
        "st",
        &["st0=0@18:4", "st1=1@19:2", "st2=2@20:2"],
        &[
            "st0>st1@18:2",
            "st1>st0@16:0",
            "st1>st2@19:2",
            "st2>st0@20:2",
        ],
    ),
    (
        "tb.ub",
        "st",
        &["st0=0@18:4", "st1=1@19:2", "st2=3@20:2"],
        &[
            "st0>st1@18:2",
            "st1>st0@16:0",
            "st1>st2@19:2",
            "st2>st0@20:2",
        ],
    ),
    (
        "tb.uc",
        "st",
        &["st0=0@18:4", "st1=1@19:3", "st2=3@20:3"],
        &[
            "st0>st1@18:3",
            "st1>st0@16:0",
            "st1>st2@19:3",
            "st2>st0@20:2",
        ],
    ),
    (
        "tb.uf",
        "st",
        &["st0=0@8:4", "st1=1@9:2", "st2=3@10:2"],
        &["st0>st1@8:2", "st1>st0@6:0", "st1>st2@9:2", "st2>st0@10:2"],
    ),
];

const SIGNED: &str = r#"// Signed constants: a comparison zero-extends a signed constant against an
// unsigned state, an assignment sign-extends it.
module sgn(input logic clk, rst, go);
  localparam logic signed [1:0] K = 2'b11;
  localparam logic [2:0] A = 3'd0, B = 3'd1;
  logic [2:0] st;
  always_ff @(posedge clk)
    if (rst) st <= A;
    else if (st == A) begin if (go) st <= B; end
    else if (st == B) st <= 3'd3;
    else if (st == K) st <= K;
    else st <= A;
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0;
  always #5 clk = ~clk;
  sgn us(.*);
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    repeat (2) begin
      go = 1; @(negedge clk); go = 0;
      repeat (4) @(negedge clk);
    end
    $finish;
  end
endmodule
"#;

const SIGNED_FSMS: &[Expect] = &[(
    "tb.us",
    "st",
    &["A=0@9:4", "B=1@10:2", "K=3@8:2"],
    &["A>B@9:2", "B>A@8:0", "B>K@10:2", "K>A@8:0"],
)];

const VALUES: &str = r#"// Which tests make a register an FSM: a test of it that controls nothing
// assigned to it (p1, p3), and one that does (p2, p4).
module p1(input logic clk, rst, input logic [1:0] sel, output logic flag);
  logic [2:0] v;        // value register, tested only to drive flag
  always_ff @(posedge clk)
    if (rst) v <= 3'd0;
    else case (sel)
      2'd0: v <= 3'd1;
      2'd1: v <= 3'd2;
      default: v <= 3'd4;
    endcase
  always_ff @(posedge clk) flag <= (v == 3'd2);
endmodule
module p2(input logic clk, rst, input logic [1:0] sel);
  logic [2:0] v;        // as p1, plus an assignment under a test of v
  always_ff @(posedge clk)
    if (rst) v <= 3'd0;
    else if (v == 3'd4) v <= 3'd0;
    else case (sel)
      2'd0: v <= 3'd1;
      2'd1: v <= 3'd2;
      default: v <= 3'd4;
    endcase
endmodule
module p3(input logic clk, rst, input logic [1:0] sel, output logic flag);
  logic [2:0] v, nv;    // two-process value register, tested for another output
  always_ff @(posedge clk) if (rst) v <= 3'd0; else v <= nv;
  always_comb begin
    nv = v;
    flag = 1'b0;
    case (sel)
      2'd0: nv = 3'd1;
      2'd1: nv = 3'd2;
      default: ;
    endcase
    if (v == 3'd2) flag = 1'b1;
  end
endmodule
module p4(input logic clk, rst, input logic [1:0] sel, output logic flag);
  logic [2:0] v, nv;    // as p3, with nv assigned under the test of v
  always_ff @(posedge clk) if (rst) v <= 3'd0; else v <= nv;
  always_comb begin
    nv = v;
    flag = 1'b0;
    case (sel)
      2'd0: nv = 3'd1;
      2'd1: nv = 3'd2;
      default: ;
    endcase
    if (v == 3'd2) begin flag = 1'b1; nv = 3'd0; end
  end
endmodule
module p5(input logic clk, rst, input logic [1:0] sel, output logic flag);
  logic [2:0] v;        // value register tested in its own block, controlling another register
  always_ff @(posedge clk)
    if (rst) begin v <= 3'd0; flag <= 1'b0; end
    else begin
      if (v == 3'd2) flag <= 1'b1;
      case (sel)
        2'd0: v <= 3'd1;
        2'd1: v <= 3'd2;
        default: v <= 3'd4;
      endcase
    end
endmodule
module tb;
  logic clk = 0, rst = 1;
  logic [1:0] sel = 0;
  logic f1, f3, f4, f5;
  always #5 clk = ~clk;
  p1 u1(.clk, .rst, .sel, .flag(f1));
  p2 u2(.clk, .rst, .sel);
  p3 u3(.clk, .rst, .sel, .flag(f3));
  p4 u4(.clk, .rst, .sel, .flag(f4));
  p5 u5(.clk, .rst, .sel, .flag(f5));
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    repeat (8) begin @(negedge clk); sel = sel + 1; end
    $finish;
  end
endmodule
"#;

const VALUES_FSMS: &[Expect] = &[
    (
        "tb.u2",
        "v",
        &["st1=0@20:4", "st2=1@21:2", "st3=2@20:2", "st0=4@18:2"],
        &[
            "st1>st2@20:2",
            "st1>st3@21:0",
            "st1>st0@22:0",
            "st2>st1@17:0",
            "st2>st3@21:2",
            "st2>st0@22:0",
            "st3>st1@17:0",
            "st3>st2@20:0",
            "st3>st0@22:2",
            "st0>st1@18:2",
        ],
    ),
    (
        "tb.u4",
        "v",
        &["st1=0@47:6", "st2=1@41:2", "st0=2@50:2"],
        &[
            "st1>st2@46:2",
            "st1>st0@47:0",
            "st2>st1@41:0",
            "st2>st0@47:2",
            "st0>st1@50:2",
            "st0>st2@46:0",
        ],
    ),
];

const HOLDS: &str = r#"// A test of the state that controls only a hold of it.
module h1(input logic clk, rst, go);
  logic [1:0] st, ns;
  always_ff @(posedge clk) if (rst) st <= 2'd0; else st <= ns;
  always_comb begin
    ns = go ? 2'd1 : 2'd0;
    if (st == 2'd1) ns = st;
  end
endmodule
module h2(input logic clk, rst, go);
  logic [1:0] st;
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else if (st == 2'd2) st <= st;
    else st <= go ? 2'd2 : 2'd1;
endmodule
module h3(input logic clk, rst, go);
  logic [1:0] st, ns;
  always_ff @(posedge clk) if (rst) st <= 2'd0; else st <= ns;
  always_comb begin
    ns = go ? 2'd1 : 2'd2;
    case (st)
      2'd1: ns = st;
      default: ;
    endcase
  end
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0;
  always #5 clk = ~clk;
  h1 u1(.*); h2 u2(.*); h3 u3(.*);
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    repeat (3) begin go = ~go; repeat (2) @(negedge clk); end
    $finish;
  end
endmodule
"#;

const HOLDS_FSMS: &[Expect] = &[
    (
        "tb.u1",
        "st",
        &["st1=0@6:2", "st0=1@4:3"],
        &["st1>st0@6:1", "st0>st1@4:0"],
    ),
    (
        "tb.u2",
        "st",
        &["st2=0@15:2", "st1=1@15:0", "st0=2@13:4"],
        &[
            "st2>st1@15:0",
            "st2>st0@15:1",
            "st1>st2@13:0",
            "st1>st0@15:0",
            "st0>st2@13:0",
        ],
    ),
    (
        "tb.u3",
        "st",
        &["st1=0@21:2", "st0=1@19:3", "st2=2@19:0"],
        &[
            "st1>st0@21:1",
            "st1>st2@21:0",
            "st0>st1@19:0",
            "st0>st2@21:0",
            "st2>st1@19:0",
            "st2>st0@21:0",
        ],
    ),
];

const PARAM_BRANCHES: &str = r#"// Branches on parameters: only the branch the parameters select is code.
module k1 #(parameter int MODE = 1, parameter int STOPS = 1) (input logic clk, rst, go);
  typedef enum logic [2:0] {IDLE, A, B, C, D} st_t;
  st_t cs, ns;
  always_ff @(posedge clk) if (rst) cs <= IDLE; else cs <= ns;
  always_comb begin
    ns = cs;
    case (cs)
      IDLE: if (go) ns = A;
      A: if (MODE != 0) ns = B; else ns = C;
      B: ns = (STOPS == 1) ? IDLE : D;
      C: ns = IDLE;
      D: ns = IDLE;
    endcase
  end
endmodule
module k2 #(parameter logic [1:0] SEL = 2'd2) (input logic clk, rst, go);
  logic [1:0] st;
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: case (SEL)
              2'd1: st <= 2'd2;
              2'd2: st <= 2'd3;
              default: st <= 2'd0;
            endcase
      default: st <= 2'd0;
    endcase
endmodule
module k3 #(parameter logic [7:0] P = 8'd255) (input logic clk, rst, go);
  logic [1:0] st;         // the selector sums at the labels' width
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: case (P + 8'd1)
              256: st <= 2'd2;
              default: st <= 2'd3;
            endcase
      default: st <= 2'd0;
    endcase
endmodule
module k4 #(parameter logic [1:0] P = 2'd0) (input logic clk, rst, go);
  logic [1:0] st;         // a label wider than 64 bits after the one that matches
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: case (P)
              2'd0: st <= 2'd2;
              128'd1: st <= 2'd3;
              default: st <= 2'd0;
            endcase
      default: st <= 2'd0;
    endcase
endmodule
module k5 #(parameter logic [1:0] P = 2'd0) (input logic clk, rst, go, input logic [1:0] v);
  localparam logic [1:0] S0 = 2'd0, S1 = 2'd1, S2 = 2'd2, S3 = 2'd3;
  logic [1:0] st;         // a label that is not constant: not folded
  always_ff @(posedge clk)
    if (rst) st <= S0;
    else case (st)
      S0: if (go) st <= S1;
      S1: case (P)
            2'd0: st <= S2;
            v: st <= S3;
            default: st <= S0;
          endcase
      default: st <= S0;
    endcase
endmodule
module k6 #(parameter logic [1:0] P = 2'b10) (input logic clk, rst, go);
  logic [1:0] st;         // casez wildcards
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: casez (P)
              2'b0?: st <= 2'd3;
              2'b1?: st <= 2'd2;
              default: st <= 2'd0;
            endcase
      default: st <= 2'd0;
    endcase
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0;
  logic [1:0] v = 2'd3;
  always #5 clk = ~clk;
  k1 #(.MODE(1), .STOPS(1)) ua(.*);
  k1 #(.MODE(0), .STOPS(2)) ub(.*);
  k2 #(.SEL(2'd2)) uc(.*);
  k3 ud(.*);
  k4 ue(.*);
  k5 uf(.*);
  k6 ug(.*);
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    repeat (4) begin go = 1; @(negedge clk); go = 0; repeat (3) @(negedge clk); end
    $finish;
  end
endmodule
"#;

const PARAM_BRANCHES_FSMS: &[Expect] = &[
    (
        "tb.ua",
        "cs",
        &[
            "IDLE=0@9:10",
            "A=1@10:4",
            "B=2@11:4",
            "C=3@12:0",
            "D=4@13:0",
        ],
        &[
            "IDLE>A@9:4",
            "A>IDLE@5:0",
            "A>B@10:4",
            "B>IDLE@11:4",
            "C>IDLE@12:0",
            "D>IDLE@13:0",
        ],
    ),
    (
        "tb.ub",
        "cs",
        &[
            "IDLE=0@9:10",
            "A=1@10:4",
            "B=2@11:0",
            "C=3@12:4",
            "D=4@13:0",
        ],
        &[
            "IDLE>A@9:4",
            "A>IDLE@5:0",
            "A>C@10:4",
            "B>IDLE@5:0",
            "B>D@11:0",
            "C>IDLE@12:4",
            "D>IDLE@13:0",
        ],
    ),
    (
        "tb.uc",
        "st",
        &["st0=0@22:6", "st1=1@23:4", "st2=3@28:4"],
        &[
            "st0>st1@22:4",
            "st1>st0@20:0",
            "st1>st2@25:4",
            "st2>st0@28:4",
        ],
    ),
    (
        "tb.ud",
        "st",
        &["st0=0@36:6", "st1=1@37:4", "st2=2@41:4"],
        &[
            "st0>st1@36:4",
            "st1>st0@34:0",
            "st1>st2@38:4",
            "st2>st0@41:4",
        ],
    ),
    (
        "tb.ue",
        "st",
        &["st0=0@49:6", "st1=1@50:4", "st2=2@55:4"],
        &[
            "st0>st1@49:4",
            "st1>st0@47:0",
            "st1>st2@51:4",
            "st2>st0@55:4",
        ],
    ),
    (
        "tb.uf",
        "st",
        &["S0=0@64:6", "S1=1@65:4", "S2=2@70:4", "S3=3@70:0"],
        &[
            "S0>S1@64:4",
            "S1>S0@68:0",
            "S1>S2@66:4",
            "S1>S3@67:0",
            "S2>S0@70:4",
            "S3>S0@70:0",
        ],
    ),
    (
        "tb.ug",
        "st",
        &["st0=0@78:6", "st1=1@79:4", "st2=2@84:4"],
        &[
            "st0>st1@78:4",
            "st1>st0@76:0",
            "st1>st2@81:4",
            "st2>st0@84:4",
        ],
    ),
];

const NOT_CONSTANT: &str = r#"// Next-state variables with assignments that are not constants.
module q1(input logic clk, rst, go, input logic [1:0] din);
  logic [1:0] cs, ns;     // two-process; one arm loads an input
  always_ff @(posedge clk) if (rst) cs <= 2'd0; else cs <= ns;
  always_comb begin
    ns = cs;
    case (cs)
      2'd0: if (go) ns = 2'd1;
      2'd1: ns = din;
      2'd2: ns = 2'd0;
      default: ns = 2'd0;
    endcase
  end
endmodule
module q2(input logic clk, rst, go);
  logic [2:0] r, nf;      // a register loaded from a counter in its own block
  always @(posedge clk or posedge rst)
    if (rst) begin r <= 3'd7; nf <= 3'd0; end
    else if (go) begin r <= 3'd7; nf <= 3'd0; end
    else if (r == 3'd7) begin r <= nf; nf <= nf + 3'd1; end
endmodule
module q3(input logic clk, rst, go);
  logic [1:0] cs, ns, cnt;   // two-process; the next state loads a counter
  always_ff @(posedge clk) if (rst) cnt <= 2'd0; else cnt <= cnt + 2'd1;
  always_ff @(posedge clk) if (rst) cs <= 2'd0; else cs <= ns;
  always_comb begin
    ns = cs;
    if (cs == 2'd0) ns = cnt;
    else if (go) ns = 2'd0;
  end
endmodule
module q4(input logic clk, rst, go, input logic [1:0] din);
  logic [1:0] st;         // one-process; one arm loads an input
  always_ff @(posedge clk)
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: st <= din;
      default: st <= 2'd0;
    endcase
endmodule
module tb;
  logic clk = 0, rst = 1, go = 0;
  logic [1:0] din = 0;
  always #5 clk = ~clk;
  q1 u1(.*); q2 u2(.clk, .rst, .go); q3 u3(.clk, .rst, .go); q4 u4(.*);
  initial begin
    repeat (2) @(negedge clk);
    rst = 0;
    repeat (6) begin go = ~go; din = din + 1; repeat (3) @(negedge clk); end
    $finish;
  end
endmodule
"#;

const NOT_CONSTANT_FSMS: &[Expect] = &[];

/// A two-process FSM (`state <= next` and an `always_comb` next-state
/// `case`): the reset adds a transition to CLOSED from every other state,
/// held states count on the clock that enters them and the next one (OPEN,
/// held three clocks, counts 2), and the totals pool states and
/// transitions.
#[test]
fn two_process_enum() {
    let json = check("door", DOOR, DOOR_FSMS);
    assert!(json.contains("\"kinds\": [\"fsm\"]"), "{json}");
    assert!(
        json.contains(
            "\"fsm\": {\"covered\": 8, \"total\": 11, \"percent\": 72.73, \
             \"states\": {\"covered\": 4, \"total\": 4, \"percent\": 100.00}, \
             \"transitions\": {\"covered\": 4, \"total\": 7, \"percent\": 57.14}}"
        ),
        "{json}"
    );
    assert!(
        json.contains(
            "{\"variable\": \"state\", \"next\": \"next\", \"file\": \"t.sv\", \"line\": 4, \
             \"reset\": [\"CLOSED\"], \"states\": ["
        ),
        "{json}"
    );
    assert!(
        json.contains("\"unreachable\": [], \"illegal\": 0}"),
        "{json}"
    );
}

/// A `default` arm moves the states no item lists (S3); a state no
/// transition reaches stays a bin (S4) and is listed as unreachable; an enum
/// member nothing names (S5) is no state; `ns = S0` in S0 is no transition.
#[test]
fn default_arm_unreachable_and_reset() {
    let json = check("default", DEFAULT_ARM, DEFAULT_ARM_FSMS);
    assert!(json.contains("\"unreachable\": [\"S4\"]"), "{json}");
}

/// A one-process FSM with localparam states and an asynchronous reset: it
/// counts only the clocks that assign the state, and two assignments in one
/// clock (`st <= A; if (x) st <= C;`) count once, as the final value.
#[test]
fn one_process_async_reset() {
    check("one", ONE_PROCESS, ONE_PROCESS_FSMS);
}

/// `define states and a next-state `if`/`else if` chain; states with no
/// name are `st0`, `st1`, ... in value order.
#[test]
fn define_encoding_and_if_chain() {
    check("ifchain", IF_CHAIN, IF_CHAIN_FSMS);
}

/// A `?:` of constants is two transitions; `c + 1` under `case (c)` yields
/// a constant per item, so a counter decoded by a `case` is an FSM; a 1-bit
/// variable is not.
#[test]
fn ternary_and_counter() {
    check("ternary", TERNARY, TERNARY_FSMS);
}

/// Parameter states, a clear outside the `case` (a transition from every
/// state, behind the ones inside the `case`), and an FSM with no reset
/// whose first value comes from an initial block.
#[test]
fn parameters_clear_and_no_reset() {
    check("params", PARAMS, PARAMS_FSMS);
}

/// Visit counts of two-process (`u2`), one-process (`u1`) and one-process
/// with a hold assignment (`uh`) FSMs under six stimuli: a clock counts only
/// when something the state process reads or writes changed since its
/// previous clock. An input toggling every clock (lane 1's `c`, lane 5's
/// `a`) makes a hold assignment count every clock.
#[test]
fn visit_counts() {
    check("visits", VISITS, VISITS_FSMS);
}

/// The forms the reference simulator recognizes (an enum cast of a
/// constant, two states, `unique case` with a multi-label item, a
/// continuous-assignment next state, `next = I` defaults, package enums, an
/// `integer` state, FSMs in a generate loop) and the ones it does not: a
/// register no `case` or `if` tests (`ua.mode`), a cast of a `?:`
/// (`uc.st`) and a state loaded from an input (`ug.st`).
#[test]
fn styles_and_rejections() {
    check("styles", STYLES, STYLES_FSMS);
}

/// An asynchronous reset that a flop drives rises in the same time slot as
/// the clock edge, after the state process ran for the edge: the run it
/// starts counts, and the next clock run compares its inputs with the
/// previous clock run's, not the reset run's.
#[test]
fn async_reset_runs() {
    check("async", ASYNC, ASYNC_FSMS);
}

/// An unbased unsized literal is its value in the state variable's width
/// (`'1` is state 3 of a 2-bit variable), and states are named `st0`,
/// `st1`, ... unless every state has a parameter's name. Instances of one
/// FSM whose parameters give different states stay separate in the design
/// unit's entry; those with the same states add up.
#[test]
fn fill_literals_and_parameterized_instances() {
    let json = check("fill", FILL, FILL_FSMS);
    let units = &json[json.find("\"design_units\"").unwrap()..];
    let unit = &units[units.find("\"design_unit\": \"shaped\"").unwrap()..];
    let unit = &unit[unit.find("\"fsms\": [").unwrap()..];
    let unit = &unit[..unit.find('\n').unwrap()];
    assert_eq!(unit.matches("{\"variable\": \"st\"").count(), 2, "{unit}");
    // ua alone, then ub and uc together.
    assert!(
        unit.contains("{\"name\": \"st2\", \"value\": 2, \"line\": 20, \"count\": 2}"),
        "{unit}"
    );
    assert!(
        unit.contains("{\"name\": \"st2\", \"value\": 3, \"line\": 20, \"count\": 5}"),
        "{unit}"
    );
}

/// A signed constant narrower than the state is its own bits: `K` of
/// `2'b11` is state 3 in `st == K` (§11.8.1: the comparison is unsigned)
/// and also in `st <= K`, which the reference simulator takes as a hold of
/// state 3 although the assignment writes 7 (§10.7), a value outside the
/// state set.
#[test]
fn signed_constants() {
    let json = check("signed", SIGNED, SIGNED_FSMS);
    assert!(json.contains("\"illegal\": 2}"), "{json}");
}

/// A register assigned constants from a `case` on another signal is a value,
/// not a state, even when something compares it with a constant (`u1`, `u3`,
/// `u5`). It is an FSM once one of its assignments, or one of its next
/// value's, is made under a test of it (`u2`, `u4`), as in the reference
/// simulator. Unnamed states are numbered as it lists them: compared
/// constants first. (Lines of states whose transitions are all made outside
/// a test of the variable are not compared; see the coverage guide.)
#[test]
fn value_registers() {
    check_lines("values", VALUES, VALUES_FSMS, false);
}

/// A test of the state that controls only a hold of it (`if (st == 1)
/// ns = st;`, a `case` item that keeps the state) makes an FSM, as in the
/// reference simulator; the hold adds no transition.
#[test]
fn held_states() {
    check("holds", HOLDS, HOLDS_FSMS);
}

/// An `if`, `?:` or `case` that only parameters decide contributes only
/// the branch they select (`A -> C` is no transition when `MODE` is 1, nor
/// `B -> D` when `STOPS` is 1); the states the other branches name stay
/// states when a `case` lists them. A `case` on parameters matches as in
/// simulation: at the widest of its selector and labels (§12.5: `P + 8'd1`
/// with `P` 255 is 256), past a label wider than 64 bits, with `casez`
/// wildcards; one with a label that is not constant is not folded. As in
/// the reference simulator.
#[test]
fn parameter_branches() {
    check("parameter_branches", PARAM_BRANCHES, PARAM_BRANCHES_FSMS);
}

/// A next state loaded from an input or a counter, in a two-process FSM,
/// a one-process one or a register's own block, makes no FSM, as in the
/// reference simulator.
#[test]
fn non_constant_next_states() {
    check("non_constant_next_states", NOT_CONSTANT, NOT_CONSTANT_FSMS);
}

/// The transition into the first assignment starts from the value the
/// variable had at the end of time 0 (a declaration initializer, an initial
/// block), and a write outside the state process is not seen: `ut` goes
/// B, then C by the testbench, then A, which is no transition.
#[test]
fn previous_state() {
    check("previous", PREVIOUS, PREVIOUS_FSMS);
}

/// FSM coverage is one of the code coverage kinds: in `all` and in bare
/// `--code-coverage`, `+cover` and `+cover=f`, and absent otherwise; the
/// simulation's output is the same with and without it.
#[test]
fn kinds_and_spellings() {
    let d = scratch("kinds");
    let kinds = |args: &[&str]| {
        let out = run(&d, DOOR, args, &[]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let json = results(&d);
        let start = json.find("\"kinds\": [").unwrap() + "\"kinds\": [".len();
        let k = json[start..start + json[start..].find(']').unwrap()].to_string();
        (k, json, out)
    };
    let all = "\"statement\", \"branch\", \"toggle\", \"fsm\"";
    for args in [
        &["--code-coverage"][..],
        &["--code-coverage=all"],
        &["+cover"],
    ] {
        let (k, json, _) = kinds(args);
        assert_eq!(k, all, "{args:?}");
        assert!(
            json.contains("\"fsms\": [{\"variable\": \"state\""),
            "{json}"
        );
    }
    let (k, _, out) = kinds(&["+cover=f"]);
    assert_eq!(k, "\"fsm\"");
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (k, json, _) = kinds(&["--code-coverage=stmt,toggle"]);
    assert_eq!(k, "\"statement\", \"toggle\"");
    assert!(!json.contains("\"fsm"), "{json}");
    let _ = std::fs::remove_file(d.join("cov.json"));
    let off = run(&d, DOOR, &[], &[]);
    assert!(!d.join("cov.json").exists());
    let on = run(&d, DOOR, &["--code-coverage=fsm"], &[]);
    assert_eq!(off.status.code(), on.status.code());
    assert_eq!(off.stdout, on.stdout);
    assert_eq!(off.stderr, on.stderr);
    let _ = std::fs::remove_dir_all(&d);
}

/// The two-state executors re-run a block on the four-state VM when it
/// reads x; FSM counts come out the same either way.
#[test]
fn counts_do_not_depend_on_executor() {
    let d = scratch("engine");
    let fsm = |env: &[(&str, &str)]| {
        let out = run(&d, VISITS, &["--code-coverage=fsm"], env);
        assert!(out.status.success());
        fsms(&results(&d))
    };
    assert_eq!(fsm(&[]), fsm(&[("XEZIM_TWO_STATE", "0")]));
    let _ = std::fs::remove_dir_all(&d);
}
