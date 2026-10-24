//! §7.10.1, §8.4: `q[$].m = v` writes member `m` of the object held by the
//! queue's last element. riscv-dv's Ibex breakpoint stream labels its last
//! instruction with `instr_list[$].label = ...`; the write was dropped, so the
//! program referenced a label that was never defined.
#[test]
fn dollar_index_writes_member_of_last_handle() {
    let result = xezim::simulate(
        r#"
class item;
  string label;
  int v;
endclass
class base;
  item q[$];
endclass
class stream extends base;
  function void fill();
    item a = new, b = new, c = new;
    q.push_back(a); q.push_back(b); q.push_back(c);
    q[$].label = "last";
    q[$].v = 7;
    q[$-1].v = 6;
    this.q[$].v++;
    begin
      item lq[$];
      item d = new;
      lq.push_back(d);
      lq[$].v = 9;
      $display("local %0d", d.v);
    end
  endfunction
endclass
module t;
  item m[$];
  initial begin
    stream s = new();
    item x = new;
    s.fill();
    $display("member %s %0d %0d %0d", s.q[2].label, s.q[2].v, s.q[1].v, s.q[0].v);
    s.q[$].label = "outer";
    $display("outer %s", s.q[2].label);
    m.push_back(x);
    m[$].v = 3;
    $display("module %0d", x.v);
    $finish;
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
    assert_eq!(
        lines,
        ["local 9", "member last 8 6 0", "outer outer", "module 3"]
    );
}
