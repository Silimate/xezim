//! §7.4.6, §7.10.1: a slice of a queue or array reached through an object
//! (`obj.q[a:b]`, `this.q[a:b]`, `arr[i].q[a:b]`, `get_obj().q[a:b]`) is the
//! elements it selects, assigned to a queue or as an operand of an unpacked
//! array concatenation. It read as no elements, or as one zero element. An
//! indexed slice (`q[i+:2]`, `q[i-:2]`, §11.5.1) names its first or last
//! element and a width; its width was read as the last index. A call
//! receiver is evaluated once (§11.3.2).

use xezim::simulate;

fn lines(src: &str) -> Vec<String> {
    simulate(src, 100)
        .expect("simulate failed")
        .output
        .iter()
        .map(|o| o.message.clone())
        .collect()
}

#[test]
fn member_slices_assign_and_concatenate() {
    let src = r#"
module tb;
  class C;
    int ql[$];
    int q2[$];
    int da[];
    int fa[4];
    function void show();
      int a[$], b[$], c[$];
      a = ql[1:2];
      b = this.ql[0:1];
      c = {ql[1:$], this.ql[0:0]};
      this.q2 = ql[0:1];
      $display("method a=%p b=%p c=%p q2=%p", a, b, c, q2);
    endfunction
  endclass
  C obj;
  C arr[2];
  int gets, i;
  int qe[$], qf[$], qg[$], qh[$], qi[$], qj[$], qk[$], qm[$];
  function C get_obj();
    gets++;
    return obj;
  endfunction
  initial begin
    obj = new;
    obj.ql = {7, 8, 9};
    obj.da = new[3];
    obj.da = '{4, 5, 6};
    obj.fa = '{1, 2, 3, 4};
    arr[1] = obj;
    qe = {obj.ql[0:1]};
    qf = obj.ql[1:2];
    qg = obj.ql[1:$];
    qh = {obj.da[1:2], obj.fa[2:3]};
    i = 1;
    qi = arr[i].ql[0:1];
    $display("qe=%p qf=%p qg=%p qh=%p qi=%p", qe, qf, qg, qh, qi);
    gets = 0;
    qj = get_obj().ql[0:1];
    $display("qj=%p gets=%0d", qj, gets);
    gets = 0;
    qk = {get_obj().ql[1:2], 5};
    $display("qk=%p gets=%0d", qk, gets);
    qm = obj.ql[0:1];
    qm = {qm, obj.ql[2:2]};
    $display("qm=%p", qm);
    obj.show();
    obj.q2 = obj.ql[1:2];
    $display("target q2=%p", obj.q2);
  end
endmodule
"#;
    let out = lines(src);
    for want in [
        "qe='{7, 8} qf='{8, 9} qg='{8, 9} qh='{5, 6, 3, 4} qi='{7, 8}",
        "qj='{7, 8} gets=1",
        "qk='{8, 9, 5} gets=1",
        "qm='{7, 8, 9}",
        "method a='{8, 9} b='{7, 8} c='{8, 9, 7} q2='{7, 8}",
        "target q2='{8, 9}",
    ] {
        assert!(
            out.iter().any(|m| m == want),
            "missing {want:?}; got {out:?}"
        );
    }
}

#[test]
fn indexed_and_out_of_range_slices() {
    let src = r#"
module tb;
  class C;
    int ql[$];
  endclass
  C obj;
  int q[$], i;
  int x1[$], x2[$], x3[$], x4[$], x5[$], x6[$];
  initial begin
    obj = new;
    obj.ql = {7, 8, 9};
    q = {10, 11, 12, 13};
    i = 0;
    x1 = q[i+:2];
    x2 = q[2-:2];
    x3 = {q[i+:2], 99};
    x4 = q[3:1];
    x5 = obj.ql[2:5];
    x6 = {obj.ql[i+:2], obj.ql[$-:1]};
    $display("x1=%p x2=%p x3=%p x4=%p x5=%p x6=%p", x1, x2, x3, x4, x5, x6);
  end
endmodule
"#;
    let out = lines(src);
    let want = "x1='{10, 11} x2='{11, 12} x3='{10, 11, 99} x4='{} x5='{9} x6='{7, 8, 9}";
    assert!(
        out.iter().any(|m| m == want),
        "missing {want:?}; got {out:?}"
    );
}

/// §11.3.2: a part-select through a call receiver of a member that is a
/// packed vector here but a queue in another class makes the call once.
/// §7.6: a slice assigned to a descending fixed array fills it from its left
/// bound.
#[test]
fn packed_member_receiver_and_descending_target() {
    let src = r#"
module tb;
  class C;
    bit [7:0] vec;
    int ql[$];
  endclass
  class D;
    int vec[$];
  endclass
  C obj;
  D other;
  int gets;
  bit [3:0] qv[$];
  int q[$];
  int dst[1:0], dst2[1:0];
  function C get_obj();
    gets++;
    return obj;
  endfunction
  initial begin
    obj = new;
    other = new;
    obj.vec = 8'ha5;
    obj.ql = {7, 8};
    q = {3, 4};
    gets = 0;
    qv = {get_obj().vec[7:4], get_obj().vec[3:0]};
    $display("qv=%0d,%0d,%0d gets=%0d", qv.size(), qv[0], qv[1], gets);
    dst = obj.ql[0:1];
    dst2 = q[0:1];
    $display("dst=%0d,%0d dst2=%0d,%0d", dst[1], dst[0], dst2[1], dst2[0]);
  end
endmodule
"#;
    let out = lines(src);
    for want in ["qv=2,10,5 gets=2", "dst=7,8 dst2=3,4"] {
        assert!(
            out.iter().any(|m| m == want),
            "missing {want:?}; got {out:?}"
        );
    }
}

/// §7.4.6: a slice of a fixed-size array keeps every position it names in an
/// unpacked array concatenation, an element past the end reading as the
/// element default, so `q = {a[0+:3]}` on `logic [7:0] a[2]` pushes three
/// elements and the operand after it stays in place.
#[test]
fn fixed_array_slice_past_the_end_keeps_its_positions() {
    let src = r#"
module tb;
  logic [7:0] a[2] = '{8'h11, 8'h22};
  logic [7:0] q[$];
  initial begin
    q = {a[0+:3], 8'h99};
    $display("T|%0d %h %h %h %h", q.size(), q[0], q[1], q[2], q[3]);
  end
endmodule
"#;
    let out = lines(src);
    let t: Vec<_> = out.iter().filter(|l| l.starts_with("T|")).collect();
    assert_eq!(t, ["T|4 11 22 xx 99"]);
}
