//! §18.5.8.1: a class constraint `foreach (cfg.q[j])` over a non-rand queue
//! or dynamic array of a non-rand handle member iterates that object's
//! current elements. Neither the per-variable trials nor the joint solver
//! followed a loop through another object, so the body constrained nothing.
#[test]
fn constraint_foreach_handle_member_queue() {
    let result = xezim::simulate(
        r#"
class cfg_c;
  bit [4:0] held[];
  bit [4:0] extra[$];
  function new();
    held = new[3];
    held[0] = 14; held[1] = 2; held[2] = 3;
    extra.push_back(9);
  endfunction
endclass
class scalar_c;
  cfg_c cfg;
  rand bit [4:0] x;
  constraint c { foreach (cfg.held[j]) x != cfg.held[j]; }
endclass
class nested_c;
  cfg_c cfg;
  rand int unsigned num;
  rand bit [4:0] cnt[];
  rand bit [4:0] lim[];
  constraint c {
    solve num before cnt;
    solve num before lim;
    num inside {[1:2]};
    foreach (cnt[i]) {
      cnt[i] != 0;
      foreach (cfg.held[j]) {
        cnt[i] != cfg.held[j];
      }
    }
    foreach (lim[i]) {
      foreach (cfg.held[j]) {
        lim[i] != cfg.held[j];
      }
      foreach (cfg.extra[j]) lim[i] != cfg.extra[j];
    }
    cnt.size() == num;
    lim.size() == num;
  }
endclass
module t;
  function automatic bit held(bit [4:0] r);
    return r inside {14, 2, 3};
  endfunction
  initial begin
    scalar_c s = new;
    nested_c l = new;
    int bad_s = 0, bad_l = 0, fails = 0;
    s.cfg = new;
    l.cfg = new;
    repeat (200) begin
      if (!s.randomize()) fails++;
      bad_s += held(s.x);
      if (!l.randomize()) fails++;
      foreach (l.cnt[i]) bad_l += held(l.cnt[i]) || l.cnt[i] == 0;
      foreach (l.lim[i]) bad_l += held(l.lim[i]) || l.lim[i] == 9;
    end
    $display("bad_s=%0d bad_l=%0d fails=%0d", bad_s, bad_l, fails);
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
    assert_eq!(lines, ["bad_s=0 bad_l=0 fails=0"]);
}
