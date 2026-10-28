//! IEEE 1800-2017 §18.5.8.1, §12.7.3: a `foreach` constraint over an empty
//! dynamic dimension has no iterations, so it adds no constraint, even one
//! that could never hold. The solver stores a dynamic dimension as
//! `(0, size - 1)`, and the loop over a multi-dimensional array sorted those
//! bounds, so an empty one walked -1 and 0 and the constant-false body made
//! `std::randomize` fail.
//!
//! Fork patch over upstream's dynamic-row solve (6539b4a4, 270b39de); drop it
//! when upstream skips empty dynamic dimensions in the loop.

use xezim::simulate;

fn messages(sim: &xezim::compiler::Simulator) -> Vec<String> {
    sim.output.iter().map(|o| o.message.trim().to_string()).collect()
}

#[test]
fn foreach_over_an_empty_dynamic_dimension_adds_no_constraint() {
    let sim = simulate(
        r#"
class C;
  rand int a[][2];
  constraint s { a.size() == 0; }
  constraint c { foreach (a[i, j]) 0; }
endclass
class D;
  rand int a[][];
  constraint s { a.size() == 0; }
  constraint c { foreach (a[i, j]) 0; }
endclass
module top;
  int b[][2];
  int d[][];
  initial begin
    C c = new;
    D dd = new;
    int r1, r2, r3, r4;
    r1 = c.randomize();
    r2 = dd.randomize();
    r3 = std::randomize(b) with { b.size() == 0; foreach (b[i, j]) 0; };
    r4 = std::randomize(d) with { d.size() == 0; foreach (d[i, j]) 0; };
    $display("R %0d %0d %0d %0d sizes %0d %0d", r1, r2, r3, r4, b.size(), d.size());
  end
endmodule
"#,
        10,
    )
    .expect("sim");
    let msgs = messages(&sim);
    assert!(msgs.iter().any(|m| m == "R 1 1 1 1 sizes 0 0"), "{msgs:?}");
}
