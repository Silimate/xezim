//! Regression for parameterized class registry `type_id` identity and
//! `$cast` through typedef aliases (mirrors the UVM factory/registry
//! parameterized-object test, where the typedefs live at class scope and the
//! checks run inside a class method):
//!
//! - `BaseType::type_id::get()` (a class-scoped member-typedef static call
//!   reached through a parameterized-class typedef alias) must return the
//!   SAME per-specialization singleton as `RegistryType::get()`.
//! - `$cast` to a destination declared via a typedef alias to a parameterized
//!   registry must not reject the cast because the alias's `#(...)` args were
//!   dropped (the type-param comparison then compared a base name against the
//!   instance's concrete specialization and wrongly failed).
//! - The concrete registry specialization (here `oreggy#(base_class#(9))`)
//!   must register when the concrete parameterized class (`base_class#(9)`) is
//!   used via a typedef, not just the default `#(0)`.
use xezim::simulate;

fn output_of(sim: &xezim::compiler::Simulator) -> String {
    sim.output
        .iter()
        .map(|o| o.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

const SRC: &str = r#"
class base_class#( int P=0 );
  typedef oreggy#(base_class#(P)) type_id;
  static function type_id get_type();
    return type_id::get();
  endfunction
endclass
class derived_class#( int P=0 ) extends base_class#(P);
  typedef oreggy#(derived_class#(P)) type_id;
endclass

class oreggy#( type T=base_class#(0) );
  static function oreggy#(T) get();
    static oreggy#(T) m_inst;
    if (m_inst == null) m_inst = new();
    return m_inst;
  endfunction
endclass

// A minimal factory: a static queue of registered type_id handles. Mirrors
// how UVM's factory registers a wrapper singleton per specialization.
class factory;
  static oreggy registered[$];

  static function void do_register(oreggy w);
    registered.push_back(w);
  endfunction
  static function bit is_registered(oreggy w);
    for (int i=0;i<registered.size();i++) begin
      if (registered[i] === w) return 1;
    end
    return 0;
  endfunction
endclass

class test;
  typedef base_class#( 9 )        BaseType;
  typedef derived_class#( 9 )     DerivedType;
  typedef oreggy#( BaseType )     RegistryType;

  task run_phase();
    RegistryType base_regtry_inst;
    RegistryType via_type_id;
    // 1. type_id::get() via a member typedef reached through a parameterized
    //    class typedef alias must equal RegistryType::get().
    base_regtry_inst = RegistryType::get();
    via_type_id = BaseType::type_id::get();
    $display("TYPEID_SAME %0d", (base_regtry_inst!==null && base_regtry_inst===via_type_id));

    // 2. $cast to a typedef-alias-typed parameterized registry destination
    //    must succeed (the resolved `#(args)` must survive the alias).
    base_regtry_inst = null;
    if ($cast(base_regtry_inst, via_type_id))
      $display("CAST_ALIAS_OK"); else $display("CAST_ALIAS_FAIL");

    // 3. The concrete #(9) specialization registers with the factory.
    factory::do_register(BaseType::type_id::get());
    $display("REGISTERED %0d", factory::is_registered(RegistryType::get()));
  endtask
endclass

module top;
  test t;
  initial begin
    t = new();
    t.run_phase();
  end
endmodule
"#;

#[test]
fn param_registry_typeid_identity_and_cast_and_registration() {
    let out = output_of(&simulate(SRC, 100).expect("sim"));
    assert!(
        out.contains("TYPEID_SAME 1"),
        "BaseType::type_id::get() != RegistryType::get() (per-spec singleton mismatch):\n{}",
        out
    );
    assert!(
        out.contains("CAST_ALIAS_OK"),
        "$cast to typedef-alias parameterized registry destination was rejected:\n{}",
        out
    );
    assert!(
        out.contains("REGISTERED 1"),
        "concrete parameterized registry specialization #(9) not registered:\n{}",
        out
    );
}

/// §8.23, §8.25: a type parameter bound to a typedef retains the specialized
/// class's registry and executes its static method, including its side effects.
#[test]
fn inherited_type_parameter_preserves_registry_dispatch() {
    let result = xezim::simulate(
        r#"class component;
  int marker;
endclass
class registry #(type T=component);
  static function T create();
    T c=new;
    c.marker+=5;
    return c;
  endfunction
endclass
class seq_base #(type ITEM_T=int,type RSP_ITEM_T=ITEM_T) extends component;
  typedef registry #(seq_base #(.ITEM_T(ITEM_T),.RSP_ITEM_T(RSP_ITEM_T))) type_id;
  function new(); marker=42; endfunction
endclass
class item;
endclass
typedef seq_base #(.ITEM_T(item)) jtag_seq;
class default_seq extends component;
  typedef registry #(default_seq) type_id;
  function new(); marker=900; endfunction
endclass
class agent_base #(type SEQ_T=default_seq);
  SEQ_T sequencer;
  function void build();
    sequencer=SEQ_T::type_id::create();
  endfunction
endclass
class agent extends agent_base #(.SEQ_T(jtag_seq));
endclass
class direct_agent extends agent_base #(.SEQ_T(seq_base #(.ITEM_T(item))));
endclass
class plain_agent extends agent_base #(.SEQ_T(default_seq));
endclass
module top;
  initial begin
    static agent a=new;
    static direct_agent b=new;
    static plain_agent c=new;
    static jtag_seq direct=jtag_seq::type_id::create();
    a.build(); b.build(); c.build();
    $display("BOUND %0d %0d",b.sequencer.marker,c.sequencer.marker);
    $display("JTAG direct=%0d generic=%0d values=%0d/%0d",direct!=null,a.sequencer!=null,direct.marker,a.sequencer.marker);
    $finish;
  end
endmodule

"#,
        100_000,
    )
    .unwrap();
    assert!(result.output.iter().any(|line| line.message == "BOUND 47 905"), "{:?}", result.output);
    assert!(
        result
            .output
            .iter()
            .any(|line| line.message == "JTAG direct=1 generic=1 values=47/47"),
        "{:?}",
        result.output
    );
}
