//! §7.10.2.4, §7.10.2.5: `pop_front()` and `pop_back()` on a queue whose
//! elements are fixed-size unpacked arrays remove the row and return it, a
//! value of the queue's element type. The returned row prints with `%p`,
//! copies into a dynamic array or a fixed-size array of the row type, and is
//! handed back by `return q.pop_front();`. An empty queue returns a row of
//! the element type's default.

fn run(src: &str) -> Vec<String> {
    let result = xezim::simulate(src, 100_000).unwrap();
    result
        .output
        .iter()
        .map(|entry| entry.message.clone())
        .filter(|l| l.starts_with("R "))
        .collect()
}

/// The issue's repro. Expected values checked on Verilator 5.046.
#[test]
fn pop_front_and_pop_back_return_the_row() {
    let lines = run(r#"
module tb;
  typedef int pair_t[2];
  pair_t qq[$];
  initial begin
    qq.push_back('{1, 2});
    qq.push_back('{3, 4});
    qq.push_back('{5, 6});
    $display("R front=%p", qq.pop_front());
    $display("R back=%p", qq.pop_back());
    $display("R qq=%p size=%0d", qq, qq.size());
  end
endmodule
"#);
    assert_eq!(
        lines,
        [
            "R front='{1, 2}",
            "R back='{5, 6}",
            "R qq='{'{3, 4}} size=1",
        ]
    );
}

/// Rows declared in place and through a typedef, a pop into a dynamic
/// array, and a pop of an empty queue. Expected values checked on
/// Verilator 5.046.
#[test]
fn popped_rows_print_and_copy_into_a_dynamic_array() {
    let lines = run(r#"
module tb;
  typedef logic [7:0] b2_t[2];
  int qi[$][2];
  b2_t qb[$];
  int da[];
  initial begin
    qi.push_back('{1, 2});
    qi.push_back('{3, 4});
    qi.push_back('{5, 6});
    $display("R %p %p n=%0d", qi.pop_back(), qi.pop_front(), qi.size());
    da = qi.pop_front();
    $display("R da=%p size=%0d n=%0d", da, da.size(), qi.size());
    $display("R empty=%p n=%0d", qi.pop_front(), qi.size());
    qb.push_back('{8'h12, 8'h34});
    qb.push_front('{8'hab, 8'hcd});
    $display("R qb %p %p n=%0d", qb.pop_front(), qb.pop_front(), qb.size());
  end
endmodule
"#);
    assert_eq!(
        lines,
        [
            "R '{5, 6} '{1, 2} n=1",
            "R da='{3, 4} size=2 n=0",
            "R empty='{0, 0} n=0",
            "R qb '{'hab, 'hcd} '{'h12, 'h34} n=0",
        ]
    );
}

/// A popped row assigned to a fixed-size array (a descending one pairs from
/// its left bound, §7.6), returned from a function, compared with an array
/// (§11.4.5) and passed as an actual argument. The pop runs once
/// (§7.10.2.4); Verilator 5.046 evaluates it once per element of the target
/// or operand in these forms, so it is no reference for them.
#[test]
fn popped_row_assigned_returned_compared_and_passed() {
    let lines = run(r#"
module tb;
  typedef int pair_t[2];
  pair_t qq[$];
  int qr[$][2:1];
  pair_t p;
  int r[2:1];
  function automatic pair_t take_back();
    return qq.pop_back();
  endfunction
  function automatic int digits(pair_t d);
    return d[0] * 10 + d[1];
  endfunction
  initial begin
    for (int k = 0; k < 6; k++) qq.push_back('{2 * k + 1, 2 * k + 2});
    p = qq.pop_front();
    $display("R p=%p n=%0d", p, qq.size());
    p = take_back();
    $display("R p=%p n=%0d", p, qq.size());
    p = '{3, 4};
    if (qq.pop_front() == p) $display("R eq n=%0d", qq.size());
    $display("R arg=%0d n=%0d", digits(qq.pop_front()), qq.size());
    qr.push_back('{1, 2});
    r = qr.pop_front();
    $display("R r[2]=%0d r[1]=%0d n=%0d", r[2], r[1], qr.size());
  end
endmodule
"#);
    assert_eq!(
        lines,
        [
            "R p='{1, 2} n=5",
            "R p='{11, 12} n=4",
            "R eq n=3",
            "R arg=56 n=2",
            "R r[2]=1 r[1]=2 n=0",
        ]
    );
}
