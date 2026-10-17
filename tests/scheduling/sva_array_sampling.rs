//! IEEE 1800-2017 §16.5.1: every value a concurrent assertion reads is a
//! sampled value, taken in the Preponed region, and that includes the
//! elements of unpacked arrays. The assertion used to read an element after
//! the NBA region of the edge it is evaluated on, because unpacked arrays
//! were not in the set of sampled signals: `a[0] == $past(s)` with
//! `a[0] <= s` failed at every edge. A select with constant indices samples
//! its cell; one whose indices read signals samples the cell those indices'
//! sampled values name, renamed in every time slot, so a large memory is
//! never copied.
//!
//! Each design checks itself and prints its failure counts. The expected
//! lines are the commercial simulator's.

use xezim::simulate;

fn result(src: &str) -> String {
    let sim = simulate(src, 1000).expect("simulate failed");
    sim.output
        .iter()
        .map(|o| o.message.trim_end().to_string())
        .find(|l| l.starts_with("RESULT"))
        .unwrap_or_default()
}

/// A constant index, an index that changes at the same edge (`k`, sampled
/// too), an index read from another array, and an array port.
#[test]
fn elements_of_unpacked_arrays_are_sampled() {
    let src = r#"
// An array port, as in a bound checker.
module chk (input logic clk, input logic v, input logic [7:0] s,
            input logic [1:0] k, input logic [7:0] arr [0:3]);
  int fails = 0;
  a_port: assert property (@(posedge clk) v |-> arr[1] == $past(s) && arr[k - 2'd1] == $past(s))
    else fails++;
endmodule
module top;
  logic clk = 0, v = 0;
  logic [7:0] a [0:3] = '{0, 0, 0, 0};
  logic [7:0] s = 0;
  logic [1:0] k = 0;
  logic [1:0] b [0:1] = '{0, 0};
  int f_const = 0, f_var = 0, f_nest = 0, f_mix = 0, n = 0;
  always #5 clk = ~clk;
  always @(negedge clk) s <= s + 3;
  always_ff @(posedge clk) begin
    a[1] <= s;
    a[k] <= s;
    k <= k + 1;
    b[0] <= k;
    v <= 1;
  end
  chk u_chk (.clk(clk), .v(v), .s(s), .k(k), .arr(a));
  a_const: assert property (@(posedge clk) v |-> a[1] == $past(s)) else f_const++;
  a_var:   assert property (@(posedge clk) v |-> a[k - 2'd1] == $past(s)) else f_var++;
  a_nest:  assert property (@(posedge clk) v |-> a[b[0]] == $past(s)) else f_nest++;
  a_mix:   assert property (@(posedge clk) v |-> a[1] != s || s == $past(s)) else f_mix++;
  c_n: cover property (@(posedge clk) v) n++;
  initial begin
    #120;
    $display("RESULT n=%0d const=%0d var=%0d nest=%0d mix=%0d port=%0d",
             n, f_const, f_var, f_nest, f_mix, u_chk.fails);
    $finish;
  end
endmodule
"#;
    assert_eq!(result(src), "RESULT n=11 const=0 var=0 nest=0 mix=0 port=0");
}

/// A two-dimensional array (constant and variable indices), elements of an
/// unpacked array of packed vectors and selects of them, an array of
/// unpacked structs and one of packed structs, an unpacked struct member,
/// and an array in an interface, read from inside and outside it.
#[test]
fn multidimensional_struct_and_interface_arrays_are_sampled() {
    let src = r#"
interface ifc (input logic clk);
  logic [7:0] mem [4];
  logic [7:0] s;
  logic v;
  int fails = 0;
  a_in: assert property (@(posedge clk) v |-> mem[2] == $past(s)) else fails++;
endinterface
module top;
  typedef struct { logic [7:0] x; logic [7:0] y; } ust;
  typedef struct packed { logic [7:0] x; logic [7:0] y; } pst;
  logic clk = 0, v = 0;
  logic [7:0] s = 0;
  logic [7:0] m [2][3];
  logic [3:0][7:0] p [2];
  logic [7:0] q [4];
  ust sa [3];
  pst pa [3];
  ust u;
  logic [1:0] i = 0, ii = 0;
  logic j = 0, jj = 0;
  ifc bus (clk);
  int f_m = 0, f_mv = 0, f_p = 0, f_pb = 0, f_q = 0, f_sa = 0, f_pa = 0, f_u = 0, f_if = 0, n = 0;
  initial begin
    foreach (m[a, b]) m[a][b] = 0;
    foreach (p[a]) p[a] = 0;
    foreach (q[a]) q[a] = 0;
    foreach (sa[a]) begin sa[a].x = 0; sa[a].y = 0; end
    foreach (pa[a]) pa[a] = 0;
    foreach (bus.mem[a]) bus.mem[a] = 0;
    u.x = 0; u.y = 0; bus.v = 0;
  end
  assign bus.s = s;
  always #5 clk = ~clk;
  always @(negedge clk) s <= s + 3;
  always @(posedge clk) begin
    m[1][2] <= s;
    m[j][i] <= s;
    i <= (i == 2) ? 0 : i + 1;
    if (i == 2) j <= ~j;
    ii <= i;
    jj <= j;
    p[1][2] <= s;
    q[3][5:2] <= s[3:0];
    sa[2].x <= s;
    pa[1].y <= s;
    u.y <= s;
    bus.mem[2] <= s;
    bus.v <= 1;
    v <= 1;
  end
  a_m:  assert property (@(posedge clk) v |-> m[1][2] == $past(s)) else f_m++;
  a_mv: assert property (@(posedge clk) v |-> m[jj][ii] == $past(s)) else f_mv++;
  a_p:  assert property (@(posedge clk) v |-> p[1][2] == $past(s)) else f_p++;
  a_pb: assert property (@(posedge clk) v |-> p[1][2][3:0] == ($past(s) & 8'h0f)) else f_pb++;
  a_q:  assert property (@(posedge clk) v |-> q[3][5:2] == ($past(s) & 8'h0f)) else f_q++;
  a_sa: assert property (@(posedge clk) v |-> sa[2].x == $past(s)) else f_sa++;
  a_pa: assert property (@(posedge clk) v |-> pa[1].y == $past(s)) else f_pa++;
  a_u:  assert property (@(posedge clk) v |-> u.y == $past(s)) else f_u++;
  a_if: assert property (@(posedge clk) v |-> bus.mem[2] == $past(s)) else f_if++;
  c_n: cover property (@(posedge clk) v) n++;
  initial begin
    #120;
    $display("RESULT n=%0d m=%0d mv=%0d p=%0d pb=%0d q=%0d sa=%0d pa=%0d u=%0d if=%0d in=%0d",
             n, f_m, f_mv, f_p, f_pb, f_q, f_sa, f_pa, f_u, f_if, bus.fails);
    $finish;
  end
endmodule
"#;
    assert_eq!(
        result(src),
        "RESULT n=11 m=0 mv=0 p=0 pb=0 q=0 sa=0 pa=0 u=0 if=0 in=0"
    );
}

/// An index read from a sequence local variable (§16.10) or a procedure's
/// loop variable (§16.14.6.1) is only known when the property is evaluated,
/// so the whole array is sampled. The `disable iff` condition reads current
/// values (§16.12), the body sampled ones: the commercial simulator reports
/// the five failures below.
#[test]
fn indices_the_preponed_region_cannot_evaluate() {
    let src = r#"
module top;
  logic clk = 0, v = 0;
  logic [7:0] s = 0;
  logic [7:0] a [0:3] = '{0, 0, 0, 0};
  logic r [0:1] = '{0, 0};
  logic [1:0] k = 0;
  int f_lv = 0, f_proc = 0, f_dis = 0, p_dis = 0, n = 0;
  always #5 clk = ~clk;
  always @(negedge clk) s <= s + 3;
  always_ff @(posedge clk) begin
    for (int x = 0; x < 4; x++) a[x] <= s + x;
    r[0] <= s[0];
    k <= k + 1;
    v <= 1;
  end
  // The index is a local variable: assigned the sampled k at one tick, it
  // selects at the next.
  property p_lv;
    logic [1:0] t;
    (v, t = k) ##1 a[t] == $past(s) + t;
  endproperty
  a_lv: assert property (@(posedge clk) p_lv) else f_lv++;
  // The index is the procedure's loop variable.
  always @(posedge clk)
    for (int x = 0; x < 4; x++)
      a_proc: assert property (@(posedge clk) v |-> a[x] == $past(s) + x) else f_proc++;
  // The disable condition reads current values, the body sampled ones.
  a_dis: assert property (@(posedge clk) disable iff (r[0]) v |-> r[0] == 0) p_dis++; else f_dis++;
  c_n: cover property (@(posedge clk) v) n++;
  initial begin
    #120;
    $display("RESULT n=%0d lv=%0d proc=%0d dis_fail=%0d dis_pass=%0d", n, f_lv, f_proc, f_dis, p_dis);
    $finish;
  end
endmodule
"#;
    assert_eq!(result(src), "RESULT n=11 lv=1 proc=0 dis_fail=5 dis_pass=0");
}

/// A memory large enough for the packed arena. Only the cells the property
/// names are sampled: `mem[5]`, and `mem[ra]`/`mem[wa]` from the sampled
/// indices; `mem[wa]` is about to be written and still unknown.
#[test]
fn large_memory_cells_are_sampled_on_demand() {
    let src = r#"
module top;
  logic clk = 0, v = 0;
  logic [31:0] mem [0:(1<<17)-1];
  logic [16:0] wa = 0, ra = 0;
  logic [31:0] s = 1;
  int f_const = 0, f_var = 0, n = 0;
  always #5 clk = ~clk;
  always @(negedge clk) s <= s * 1103515245 + 12345;
  always_ff @(posedge clk) begin
    mem[wa] <= s;
    mem[5] <= s;
    ra <= wa;
    wa <= wa + 7919;
    v <= 1;
  end
  a_const: assert property (@(posedge clk) v |-> mem[5] == $past(s)) else f_const++;
  a_var:   assert property (@(posedge clk) v |-> mem[ra] == $past(s) && $isunknown(mem[wa])) else f_var++;
  c_n: cover property (@(posedge clk) v) n++;
  initial begin
    #200;
    $display("RESULT n=%0d const=%0d var=%0d", n, f_const, f_var);
    $finish;
  end
endmodule
"#;
    assert_eq!(result(src), "RESULT n=19 const=0 var=0");
}

/// Elements of a dynamic array and of a queue, written with blocking
/// assignments at the edge.
#[test]
fn dynamic_array_and_queue_elements_are_sampled() {
    let src = r#"
module top;
  logic clk = 0, v = 0;
  logic [7:0] s = 0;
  int da [];
  int dq [$];
  int k = 0, kk = 0;
  int f_da = 0, f_q = 0, f_qv = 0, n = 0;
  initial begin da = new[4]; dq = {0, 0, 0, 0}; end
  always #5 clk = ~clk;
  always @(negedge clk) s <= s + 3;
  // Blocking writes at the edge: the sampled values are still the old ones.
  always @(posedge clk) begin
    da[1] = s;
    dq[2] = s;
    dq[k] = s;
    kk = k;
    k = (k + 1) % 4;
    v = 1;
  end
  a_da: assert property (@(posedge clk) v |-> da[1] == $past(s)) else f_da++;
  a_q:  assert property (@(posedge clk) v |-> dq[2] == $past(s)) else f_q++;
  a_qv: assert property (@(posedge clk) v |-> dq[kk] == $past(s)) else f_qv++;
  c_n: cover property (@(posedge clk) v) n++;
  initial begin
    #120;
    $display("RESULT n=%0d da=%0d q=%0d qv=%0d", n, f_da, f_q, f_qv);
    $finish;
  end
endmodule
"#;
    assert_eq!(result(src), "RESULT n=11 da=0 q=0 qv=0");
}

/// `$past`, `$rose` and `$stable` of array elements, with constant and
/// variable indices: their history must be kept from sampled values too.
#[test]
#[ignore = "known gap: sampled-value functions of a select keep no history (#14)"]
fn sampled_value_functions_of_elements() {
    let src = r#"
module top;
  logic clk = 0, v = 0;
  logic [7:0] a [0:3] = '{0, 0, 0, 0};
  logic [7:0] s = 0;
  logic r [0:1] = '{0, 0};
  logic [7:0] h = 0;
  logic [1:0] k = 0;
  int f_past = 0, f_rose = 0, f_stab = 0, f_pv = 0, f_rv = 0, n = 0;
  always #5 clk = ~clk;
  always @(negedge clk) s <= s + 3;
  always_ff @(posedge clk) begin
    a[1] <= s;
    a[2] <= a[1];
    r[0] <= s[0];
    r[1] <= r[0];
    a[3] <= s;
    h <= a[k];
    k <= k + 1;
    v <= 1;
  end
  // a[2] and r[1] hold the samples of a[1] and r[0] from the previous tick.
  a_past: assert property (@(posedge clk) v ##1 v |-> a[2] == $past(a[1])) else f_past++;
  a_rose: assert property (@(posedge clk) v ##1 v |-> $rose(r[0]) == (r[0] && !r[1])) else f_rose++;
  a_stab: assert property (@(posedge clk) v ##1 v |-> $stable(a[1]) == (a[1] == a[2])) else f_stab++;
  // h holds the sample of a[k] from the previous tick.
  a_pv: assert property (@(posedge clk) v ##1 v |-> h == $past(a[k])) else f_pv++;
  a_rv: assert property (@(posedge clk) v ##1 v |-> $rose(a[k][0]) == (a[k][0] && !h[0])) else f_rv++;
  c_n: cover property (@(posedge clk) v) n++;
  initial begin
    #120;
    $display("RESULT n=%0d past=%0d rose=%0d stable=%0d pastvar=%0d rosevar=%0d",
             n, f_past, f_rose, f_stab, f_pv, f_rv);
    $finish;
  end
endmodule
"#;
    assert_eq!(
        result(src),
        "RESULT n=11 past=0 rose=0 stable=0 pastvar=0 rosevar=0"
    );
}
