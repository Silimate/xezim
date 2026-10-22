//! `item #(H, D) req = new;` inside `my_seq #(32, 194)` must build
//! `item #(32, 194)`, not the default specialization.
//!
//! The type arguments are the enclosing class's value parameters. They
//! already read back as 32 and 194, and the inherited `$cast` destination
//! (`RSP`, bound through a type-parameter base) is that same specialization.
//! The constructor used to keep the bare names and fall back to the
//! defaults, so `put_response` rejected its own request. UVM's
//! scrambling-key device sequence does this with `HostDataWidth` and
//! `DeviceDataWidth`.

use xezim::simulate;

fn u(sim: &xezim::compiler::Simulator, n: &str) -> u64 {
    sim.get_signal(n)
        .or_else(|| sim.get_signal(&format!("top.{n}")))
        .unwrap_or_else(|| panic!("signal not found: {n}"))
        .to_u64()
        .unwrap_or_else(|| panic!("{n} not u64-able"))
        & 0xFFFF_FFFF
}

#[test]
fn new_inside_a_method_uses_the_enclosing_value_parameters() {
    let src = r#"
class base_item;
endclass

class item #(int H = 32, int D = H) extends base_item;
  int tag;
endclass

class uvm_sequence #(type REQ = base_item, type RSP = REQ);
  virtual function int put_response(base_item response_item);
    RSP response;
    return $cast(response, response_item);
  endfunction
endclass

class mid #(type REQ = base_item, type RSP = REQ) extends uvm_sequence #(REQ, RSP);
endclass

class my_seq #(int H = 32, int D = H) extends mid #(item #(H, D), item #(H, D));
  function int check();
    item #(H, D) req;
    req = new;
    req.tag = D;
    return put_response(req);
  endfunction
  function int seen_d();
    item #(H, D) req;
    req = new;
    return req.D;
  endfunction
  function int check_lit();
    item #(32, 194) req;
    req = new;
    return put_response(req);
  endfunction
endclass

module top;
  my_seq #(32, 194) wide;
  my_seq #(32, 32) narrow;
  item #(32, 32) nitem;
  item #(32, 194) witem;
  int wide_ok, narrow_ok, swapped, explicit, lit_ok, got_d;
  initial begin
    wide = new;
    narrow = new;
    nitem = new;
    witem = new;
    got_d = wide.seen_d();
    wide_ok = wide.check();
    narrow_ok = narrow.check();
    swapped = wide.put_response(nitem);
    explicit = wide.put_response(witem);
    lit_ok = wide.check_lit();
  end
endmodule
"#;
    let sim = simulate(src, 20).expect("simulate failed");
    let got = (
        u(&sim, "wide_ok"),
        u(&sim, "narrow_ok"),
        u(&sim, "swapped"),
        u(&sim, "explicit"),
        u(&sim, "lit_ok"),
        u(&sim, "got_d"),
    );
    assert_eq!(
        got,
        (1, 1, 0, 1, 1, 194),
        "wide, narrow, swapped, explicit, lit, got_d"
    );
}

/// `resource#(T) r = new` inside `db#(virtual clk_rst_if)` must be that
/// specialization. The type argument is not a class, so it used to
/// canonicalize to the default `int`. The object's type handle then
/// disagreed with `resource#(virtual clk_rst_if)::get_type()`, which is
/// how uvm_config_db's get missed the resource set had just stored.
#[test]
fn nested_new_keeps_a_virtual_interface_type_argument() {
    let src = r#"
class resource #(type T = int);
  static resource#(T) singleton;
  static function resource#(T) get_type();
    if (singleton == null) singleton = new;
    return singleton;
  endfunction
  function resource#(T) type_handle();
    return get_type();
  endfunction
endclass

class db #(type T = int);
  static function int same();
    resource#(T) r = new;
    return r.type_handle() == resource#(T)::get_type();
  endfunction
endclass

interface clk_rst_if;
endinterface

class payload;
endclass

module top;
  int vif_ok, cls_ok;
  initial begin
    vif_ok = db#(virtual clk_rst_if)::same();
    cls_ok = db#(payload)::same();
  end
endmodule
"#;
    let sim = simulate(src, 20).expect("simulate failed");
    assert_eq!(
        (u(&sim, "vif_ok"), u(&sim, "cls_ok")),
        (1, 1),
        "virtual-interface and class type arguments"
    );
}

/// `$cast` back to `uvm_resource#(virtual mon_if)` must succeed when the
/// resource has a property of that type. The integer parameter makes the
/// specialization entry `uvm_resource<virtualmon_if#(32)>`. A `#` split
/// of that name stopped inside the signature, so the cast no longer saw
/// the object as a `uvm_resource`. UVM's get_highest_precedence does this
/// cast for `instr_monitor_if`.
#[test]
fn cast_of_integer_parameter_virtual_interface() {
    let src = r#"
class uvm_resource_base;
endclass

class uvm_resource #(type T = int) extends uvm_resource_base;
  typedef uvm_resource#(T) this_type;
  T val;
  static function uvm_resource_base make();
    uvm_resource#(T) r = new;
    return r;
  endfunction
  static function int from_q(uvm_resource_base q[$]);
    this_type rsrc;
    int first;
    for (first = 0; first < q.size() && !$cast(rsrc, q[first]); first++) ;
    return rsrc != null;
  endfunction
endclass

interface clk_rst_if #(parameter string IfName = "main");
endinterface

interface plain_if;
endinterface

interface mon_if #(parameter DATA_WIDTH = 32);
endinterface

module top;
  int clk_ok, plain_ok, mon_ok;
  uvm_resource_base cq[$], pq[$], mq[$];
  initial begin
    cq.push_back(uvm_resource#(virtual clk_rst_if)::make());
    pq.push_back(uvm_resource#(virtual plain_if)::make());
    mq.push_back(uvm_resource#(virtual mon_if)::make());
    clk_ok = uvm_resource#(virtual clk_rst_if)::from_q(cq);
    plain_ok = uvm_resource#(virtual plain_if)::from_q(pq);
    mon_ok = uvm_resource#(virtual mon_if)::from_q(mq);
  end
endmodule
"#;
    let sim = simulate(src, 20).expect("simulate failed");
    assert_eq!(
        (u(&sim, "clk_ok"), u(&sim, "plain_ok"), u(&sim, "mon_ok")),
        (1, 1, 1),
        "string-param, plain, and integer-param virtual interfaces"
    );
}
