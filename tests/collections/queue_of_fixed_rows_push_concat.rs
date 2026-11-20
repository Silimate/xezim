//! §7.10.2, §10.10: the argument of `push_back`, `push_front` and `insert`
//! on a queue whose elements are fixed-size unpacked arrays has the element
//! type, so braces (`{1, 2}`) are an unpacked array concatenation of that
//! row type: each item is one element of the new row, as with the pattern
//! `'{1, 2}`. Braces nested inside it are one item, a vector concatenation
//! (§10.10.3). Module, class-property and subroutine-local queues alike.
//! Expected values were checked on the reference simulator.

fn run(src: &str) -> Vec<String> {
    let result = xezim::simulate(src, 100_000).unwrap();
    result
        .output
        .iter()
        .map(|entry| entry.message.clone())
        .filter(|l| l.starts_with("R "))
        .collect()
}

#[test]
fn push_back_push_front_and_insert_take_a_concatenation_row() {
    let lines = run(r#"
module tb;
  typedef int pair_t[2];
  pair_t qp[$];
  pair_t p;
  initial begin
    qp.push_back({1, 2});
    qp.push_back('{3, 4});
    p = {5, 6};
    qp.push_back(p);
    qp.push_front({7, 8});
    qp.insert(1, {9, 10});
    $display("R n=%0d", qp.size());
    foreach (qp[i]) $display("R %0d: %0d %0d", i, qp[i][0], qp[i][1]);
  end
endmodule
"#);
    assert_eq!(
        lines,
        [
            "R n=5",
            "R 0: 7 8",
            "R 1: 9 10",
            "R 2: 1 2",
            "R 3: 3 4",
            "R 4: 5 6",
        ]
    );
}

#[test]
fn concatenation_items_fill_the_row_in_declared_order() {
    let lines = run(r#"
module tb;
  typedef byte b2_t[2];
  typedef logic [3:0] n2_t[2];
  typedef int r_t[2:1];
  typedef int pair_t[2];
  b2_t qb[$];
  n2_t qn[$];
  r_t qr[$];
  int qd[$][2];
  pair_t qp[$];
  int a, b, k;
  function int bump(); k++; return k; endfunction
  initial begin
    a = 11; b = 12;
    qb.push_back({300, -1});
    qb.push_front({-8'sd3, 8'd4});
    qn.push_back({4'hf, 1'b1});
    qr.push_back({1, 2});
    qr.push_front({3, 4});
    qd.push_back({a + 1, b});
    qd.insert(0, {a, b});
    qp.push_back({{8'd1, 8'd2}, 3});
    k = 0;
    qp.push_back({bump(), bump()});
    $display("R qb %0d %0d %0d %0d", qb[0][0], qb[0][1], qb[1][0], qb[1][1]);
    $display("R qn %0d %0d", qn[0][0], qn[0][1]);
    $display("R qr %0d %0d %0d %0d", qr[0][2], qr[0][1], qr[1][2], qr[1][1]);
    $display("R qd %0d %0d %0d %0d", qd[0][0], qd[0][1], qd[1][0], qd[1][1]);
    $display("R qp %0d %0d %0d %0d k=%0d", qp[0][0], qp[0][1], qp[1][0], qp[1][1], k);
  end
endmodule
"#);
    assert_eq!(
        lines,
        [
            "R qb -3 4 44 -1",
            "R qn 15 1",
            "R qr 3 4 1 2",
            "R qd 11 12 12 12",
            "R qp 258 3 1 2 k=2",
        ]
    );
}

#[test]
fn class_property_and_task_local_queues_of_rows() {
    let lines = run(r#"
module tb;
  typedef int pair_t[2];
  class K;
    pair_t q[$];
    function void fill();
      q.push_back({1, 2});
      q.push_front({3, 4});
      q.insert(1, {5, 6});
    endfunction
  endclass
  K k;
  task automatic t();
    pair_t lq[$];
    lq.push_back({1, 2});
    lq.push_front({3, 4});
    lq.insert(1, {5, 6});
    $display("R lq=%p", lq);
  endtask
  initial begin
    k = new;
    k.fill();
    k.q.push_back({7, 8});
    $display("R k.q=%p", k.q);
    t();
  end
endmodule
"#);
    assert_eq!(
        lines,
        [
            "R k.q='{'{3, 4}, '{5, 6}, '{1, 2}, '{7, 8}}",
            "R lq='{'{3, 4}, '{5, 6}, '{1, 2}}",
        ]
    );
}
