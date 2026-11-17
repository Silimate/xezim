//! IEEE 1800-2023 §26.3, §6.20: a package parameter's default is evaluated
//! in the package that declares it, so it reads the names that package
//! imports. `package z; import e::*; localparam B = DataSize / 64;` gave
//! `z::B == 0` whenever `z` happened to be evaluated before `e`, which
//! depended only on the package names, and a module that imported `z`
//! evaluated `B` in its own scope, where `DataSize` is not visible. CORE-ET's
//! `icache_pkg` (`IcacheEccBlocks = DataSize / 64` with `DataSize` from
//! `etlink_pkg`) sized `icache_err_log_info_t` from that 0. The expected
//! values are the reference simulator's.

use xezim::simulate;

fn lines(src: &str) -> Vec<String> {
    let sim = simulate(src, 100).unwrap_or_else(|e| panic!("{e}\n{src}"));
    sim.output.iter().map(|o| o.message.clone()).collect()
}

fn expect(src: &str, want: &[&str]) {
    let got = lines(src);
    for w in want {
        assert!(
            got.iter().any(|l| l == w),
            "missing {w:?} in {got:?}\n{src}"
        );
    }
}

/// Package-name pairs that cover both hash orders and both name orders.
const PAIRS: &[(&str, &str)] = &[
    ("e", "z"),
    ("etlink_pkg", "z"),
    ("e", "zpkg"),
    ("a_pkg", "b_pkg"),
    ("b_pkg", "a_pkg"),
    ("zz", "aa"),
];

fn pkgs(e: &str, z: &str) -> String {
    format!(
        "package {e}; localparam int unsigned DataSize = 512; endpackage\n\
         package {z}; import {e}::*; localparam int unsigned B = DataSize / 64;\n\
         \x20 typedef struct packed {{ logic [B-1:0] x; logic [2:0] y; }} t;\n\
         \x20 typedef logic [DataSize-1:0] line_t;\n\
         endpackage\n"
    )
}

/// §26.3: `z::B` reads `DataSize` through `z`'s wildcard import, whatever
/// the two packages are called.
#[test]
fn qualified_reference_reads_the_declaring_package_import() {
    for (e, z) in PAIRS {
        let src = format!(
            "{}module top; initial $display(\"R|%0d\", {z}::B); endmodule\n",
            pkgs(e, z)
        );
        expect(&src, &["R|8"]);
    }
}

/// §26.3: `B` reached through a module's `import z::*` is the same
/// parameter as `z::B`; its default is not re-evaluated in the module.
#[test]
fn wildcard_imported_parameter_keeps_its_package_value() {
    for (e, z) in PAIRS {
        let src = format!(
            "{}module top; import {z}::*;\n\
             \x20 initial $display(\"R|%0d %0d\", B, {z}::B);\nendmodule\n",
            pkgs(e, z)
        );
        expect(&src, &["R|8 8"]);
    }
}

/// §26.3, §6.18: a package typedef sized by such a parameter, or by the
/// imported parameter itself, has the same width through an import, a
/// qualified name and a port.
#[test]
fn typedef_widths_follow_the_imported_parameter() {
    for (e, z) in PAIRS {
        let src = format!(
            "{}module m(input {z}::t i); initial #1 $display(\"M|%0d\", $bits(i)); endmodule\n\
             module top; import {z}::*; t v; line_t l; m u(.i(v));\n\
             \x20 initial $display(\"R|%0d %0d %0d %0d %0d\", $bits(t), $bits(v), $bits({z}::t),\n\
             \x20                  $bits(l), $bits({z}::line_t));\nendmodule\n",
            pkgs(e, z)
        );
        expect(&src, &["R|11 11 11 512 512", "M|11"]);
    }
}

/// §26.3: an import chain longer than the old three evaluation passes,
/// declared so that every package sorts before the one it imports.
#[test]
fn import_chain_resolves_in_dependency_order() {
    let src = r#"
package d; localparam int W = 2; endpackage
package c; import d::*; localparam int X = W * 3; endpackage
package b; import c::*; localparam int Y = X + 1; endpackage
package a; import b::*; localparam int Z = Y * 10; endpackage
module top; import a::*; initial $display("R|%0d %0d", Z, a::Z); endmodule
"#;
    expect(src, &["R|70 70"]);
}

/// §26.3: an explicitly imported parameter is evaluated in its package too,
/// where it sees the package's explicit import and its other parameters,
/// none of which the module imports.
#[test]
fn explicitly_imported_parameter_sees_its_package_scope() {
    let src = r#"
package e; localparam int unsigned DataSize = 512; endpackage
package z;
  import e::DataSize;
  localparam int unsigned A = 4;
  localparam int unsigned B = DataSize / 64;
  localparam int unsigned C = A * B;
endpackage
module top; import z::C; initial $display("R|%0d %0d", C, z::C); endmodule
"#;
    expect(src, &["R|32 32"]);
}

/// §26.3: a package's own declaration shadows its wildcard imports, two
/// wildcard imports of one name leave it to that declaration, and a module's
/// own declarations do not leak into the package's defaults.
#[test]
fn package_scope_rules_decide_the_name() {
    let src = r#"
package e1; localparam int unsigned DataSize = 512; endpackage
package e2; localparam int unsigned DataSize = 256; endpackage
package z; import e1::*; import e2::*;
  localparam int unsigned DataSize = 128;
  localparam int unsigned B = DataSize / 64;
endpackage
package y; import e2::*; localparam int unsigned B = DataSize / 64; endpackage
package x; import e1::*; localparam int unsigned A = 4;
  localparam int unsigned B = DataSize / 64 * A;
endpackage
module top;
  localparam int unsigned DataSize = 7;
  localparam int unsigned A = 1;
  import x::*;
  initial $display("R|%0d %0d %0d %0d %0d %0d", z::B, y::B, x::B, B, DataSize, A);
endmodule
"#;
    expect(src, &["R|2 4 32 32 7 1"]);
}

/// §26.3 makes a name that two wildcard imports provide invisible. CORE-ET's
/// `icache_pkg` still reads `PaSize`, which `etlink_pkg` and `minion_pkg`
/// both declare as 40; xezim does not diagnose that in a package, and since
/// both candidates hold the same value, it evaluates the default with it,
/// through every path, as the tools that accept the reference do.
#[test]
fn same_valued_wildcard_candidates_give_that_value() {
    let src = r#"
package e1; localparam int unsigned PaSize = 40; endpackage
package e2; localparam int unsigned PaSize = 40; localparam int unsigned DataSize = 512; endpackage
package z; import e1::*; import e2::*;
  localparam int unsigned B = DataSize / 64;
  typedef struct packed { logic [B-1:0] x; logic [PaSize-1:6] a; } t;
endpackage
module top; import z::*; t v;
  initial $display("R|%0d %0d %0d", $bits(t), $bits(v), $bits(z::t));
endmodule
"#;
    expect(src, &["R|42 42 42"]);
}

/// §6.20.2: a default that names something declared nowhere is still an
/// error, including when only a typedef width reads that parameter.
#[test]
fn undeclared_name_in_package_parameter_is_an_error() {
    for body in [
        "initial $display(\"%0d\", z::B);",
        "initial $display(\"%0d\", B);",
        "initial $display(\"%0d\", $bits(t));",
    ] {
        let src = format!(
            "package e; localparam int unsigned DataSize = 512; endpackage\n\
             package z; import e::*; localparam int unsigned B = Nope / 64;\n\
             \x20 typedef struct packed {{ logic [B-1:0] x; }} t; endpackage\n\
             module top; import z::*; {body} endmodule\n"
        );
        let err = simulate(&src, 10)
            .err()
            .unwrap_or_else(|| panic!("accepted:\n{src}"));
        assert!(err.contains("Nope"), "{err}");
    }
}
