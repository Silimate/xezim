//! §8.25.1, §6.20.3: a typedef of a parameterized class that names a member
//! type of its type parameter (`class extractor #(type T); typedef T::hw_t
//! hw_t; endclass`) is that member type in the specialization a class scope
//! names, so `extractor#(cfg_t)::hw_t` is `cfg_t::hw_t`; and a type parameter
//! or typedef bound to a class names that class as a scope (`C::hw_t`)
//! (issue #389). These came out as 32-bit types, with no diagnostic, and
//! writes to the struct's members were lost.
//!
//! Expected values are Verilator's (5.046) for the first test. Verilator
//! rejects the type-parameter default of the second (`Expecting a data
//! type`), and in the third prints 128 for both instances, where §6.20.3
//! binds `C` to `cfg2_t` in `w2`, whose `hw_t` is 16 bits.

use xezim::simulate;

fn lines(src: &str) -> Vec<String> {
    simulate(src, 100)
        .expect("simulate failed")
        .output
        .iter()
        .map(|o| o.message.clone())
        .collect()
}

fn assert_has(got: &[String], want: &str) {
    assert!(
        got.iter().any(|l| l == want),
        "missing `{want}` in {got:#?}"
    );
}

const CLASSES: &str = r#"
package p;
  typedef struct packed { logic [99:0] a; logic [27:0] b; } s_t;
endpackage
class cfg_t;
  typedef p::s_t hw_t;
endclass
class cfg2_t;
  typedef logic [15:0] hw_t;
endclass
class extractor #(type T = int);
  typedef T::hw_t hw_t;
endclass
"#;

/// A variable, a typedef and a typedef of the specialization, positional
/// and named arguments.
#[test]
fn specialization_names_the_argument_member_type() {
    let got = lines(&format!(
        "{CLASSES}{}",
        r#"
module top;
  extractor#(cfg_t)::hw_t v1;
  typedef extractor#(cfg_t)::hw_t h_t;
  h_t v2;
  typedef extractor#(cfg_t) e_t;
  e_t::hw_t v3;
  extractor#(.T(cfg_t))::hw_t v4;
  extractor#(cfg2_t)::hw_t v5;
  initial begin
    v1 = '0;
    v1.a = 100'h5;
    $display("s1: v1=%0d v2=%0d v3=%0d v4=%0d v5=%0d a=%h", $bits(v1), $bits(v2),
             $bits(v3), $bits(v4), $bits(v5), v1.a);
  end
endmodule
"#
    ));
    assert_has(
        &got,
        "s1: v1=128 v2=128 v3=128 v4=128 v5=16 a=0000000000000000000000005",
    );
}

/// A type parameter's default names a typedef of a specialization by an
/// earlier type parameter, as the CHIPS Alliance I3C core's
/// `axi_adapter_wrapper.sv` does; the variable drives a 128-bit port.
#[test]
fn type_parameter_default_through_a_specialization() {
    let got = lines(&format!(
        "{CLASSES}{}",
        r#"
module sub (input p::s_t x);
  initial #1 $display("s2: sub $bits(x)=%0d x.a=%h", $bits(x), x.a);
endmodule
module wrap #(parameter type C = cfg_t, parameter type H = extractor#(C)::hw_t) ();
  H h;
  initial begin
    h = '0;
    h.a = 100'h123;
    $display("s2: wrap $bits(H)=%0d", $bits(H));
  end
  sub u (.x(h));
endmodule
module top;
  wrap w ();
endmodule
"#
    ));
    assert_has(&got, "s2: wrap $bits(H)=128");
    assert_has(&got, "s2: sub $bits(x)=128 x.a=0000000000000000000000123");
}

/// A type parameter used as a class scope, bound per instance.
#[test]
fn type_parameter_as_a_class_scope() {
    let got = lines(&format!(
        "{CLASSES}{}",
        r#"
module wrap #(parameter type C = cfg_t) ();
  C::hw_t h;
  initial #1 $display("s3: %m $bits=%0d", $bits(h));
endmodule
module top;
  wrap w ();
  wrap #(.C(cfg2_t)) w2 ();
endmodule
"#
    ));
    assert_has(&got, "s3: top.w $bits=128");
    assert_has(&got, "s3: top.w2 $bits=16");
}
