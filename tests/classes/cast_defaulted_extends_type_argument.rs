//! §8.25: a type parameter omitted from a named `extends` list takes its
//! default, and a default may name another parameter of the same class.
//!
//! `drv` extends `mid` without passing `RSP_ITEM_T`, so that parameter
//! stays `ITEM_T` and `base.rsp` is an `item`. Its class came back unknown,
//! and a local `other rsp` in an unrelated task then supplied `other`
//! through the shared bare-name class map, so the cast rejected an `item`.
//! An actual `other` must still be rejected.

use xezim::simulate;

const SRC: &str = r#"
class item;
  int x;
  function new();
    x = 4;
  endfunction
endclass

class other;
  int x;
  function new();
    x = 9;
  endfunction
endclass

class base #(type REQ = item, type RSP = REQ);
  REQ req;
  RSP rsp;
endclass

class mid #(type ITEM_T = item, type CFG_T = item, type RSP_ITEM_T = ITEM_T)
  extends base #(.REQ(ITEM_T), .RSP(RSP_ITEM_T));
endclass

class drv extends mid #(.ITEM_T(item), .CFG_T(other));
  function void take();
    item c;
    other o;
    c = new();
    o = new();
    $cast(rsp, c);
    $display("good=%0d", rsp.x);
    $cast(rsp, o);
    $display("still=%0d", rsp.x);
  endfunction
endclass

task automatic plant();
  other rsp;
  rsp = new();
endtask

module t;
  initial begin
    drv d;
    d = new();
    plant();
    d.take();
  end
endmodule
"#;

#[test]
fn cast_of_defaulted_rsp_ignores_foreign_local() {
    let out: Vec<String> = simulate(SRC, 100)
        .expect("simulate failed")
        .output
        .iter()
        .map(|o| o.message.clone())
        .collect();
    let errors: Vec<&String> = out
        .iter()
        .filter(|m| m.contains("assignment-compatible"))
        .collect();
    assert!(
        out.iter().any(|m| m.contains("good=4")),
        "compatible cast was rejected: {out:?}"
    );
    assert!(
        out.iter().any(|m| m.contains("still=4")),
        "incompatible cast overwrote rsp: {out:?}"
    );
    assert_eq!(
        errors.len(),
        1,
        "the other-to-item cast must fail once, and only once: {out:?}"
    );
}
