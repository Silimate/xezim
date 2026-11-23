//! §11.4.13: an unpacked array in an `inside` set contributes its elements,
//! and §10.10: an unpacked array operand of an unpacked array concatenation
//! contributes all of its elements. Inside a method, `pkg::arr` is that
//! package array, not one packed value.
#[test]
fn package_array_operand_in_method() {
    let result = xezim::simulate(
        r#"
package p;
  typedef enum bit [1:0] {U = 2'b00, S = 2'b01, M = 2'b11} mode_t;
  mode_t modes[] = {M, U};
  int iq[$] = {3, 7};
endpackage
class gen;
  function string f();
    int cat[$];
    cat = {p::iq, 9};
    return $sformatf("%0d%0d%0d %0d%0d %p", p::M inside {p::modes}, p::S inside {p::modes},
                     p::U inside {p::modes}, 7 inside {p::iq}, 8 inside {1, p::iq}, cat);
  endfunction
endclass
module t;
  gen g;
  initial begin
    g = new();
    $display("%s", g.f());
  end
endmodule
"#,
        100_000,
    )
    .unwrap();
    let lines: Vec<_> = result
        .output
        .iter()
        .map(|entry| entry.message.as_str())
        .collect();
    assert_eq!(lines, ["101 10 '{3, 7, 9}"]);
}

/// A method parameter named like the package is the object, so `{p.iq, 9}`
/// reads the object's queue rather than the package queue.
#[test]
fn object_named_like_package_in_method() {
    let result = xezim::simulate(
        r#"
package p;
  int iq[$] = {3, 7};
endpackage
class holder;
  int iq[$] = {1, 2};
endclass
class gen;
  function string f(holder p);
    int cat[$];
    cat = {p.iq, 9};
    return $sformatf("%p", cat);
  endfunction
endclass
module t;
  gen g;
  holder h;
  initial begin
    g = new();
    h = new();
    $display("%s", g.f(h));
  end
endmodule
"#,
        100_000,
    )
    .unwrap();
    let lines: Vec<_> = result
        .output
        .iter()
        .map(|entry| entry.message.as_str())
        .collect();
    assert_eq!(lines, ["'{1, 2, 9}"]);
}

#[test]
fn copy_fixed_package_strings_into_inherited_dynamic_array() {
    let result = xezim::simulate(
        r#"
package alpha;
  parameter string names[5:3] = '{"amber", "blue", "cyan"};
endpackage
package beta;
  parameter string names[2] = '{"north", "south"};
endpackage
class parent;
  string names[];
endclass
class cfg extends parent;
  function void run();
    names = alpha::names;
    $display("alpha %0d %s %s %s", names.size(), names[0], names[1], names[2]);
    this.names = beta::names;
    $display("beta %0d %s %s", names.size(), names[0], names[1]);
  endfunction
endclass
module top;
  initial begin
    cfg c = new;
    c.run();
  end
endmodule
"#,
        100_000,
    )
    .unwrap();
    let output: Vec<_> = result.output.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(output, ["alpha 3 amber blue cyan", "beta 2 north south"]);
}
