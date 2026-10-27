//! §7.5 / §7.6: `d[i] = src` copies a dynamic array, a queue or another row
//! into row `d[i]` of a dynamic array of dynamic arrays and resizes the row,
//! for a class property (inside a method and through a handle) and for a
//! local. The row does not alias its source.
#[test]
fn dyn_array_of_dyn_array_row_copy() {
    let result = xezim::simulate(
        r#"
class c;
  int v[][];
  function void build();
    int tmp[];
    int q[$];
    v = new[3];
    tmp = '{1, 2, 3, 4, 5};
    v[0] = tmp;
    tmp = '{8, 9};
    v[0] = tmp;
    q = '{6, 7, 8};
    v[1] = q;
    v[2] = v[1];
    tmp[0] = 42;
  endfunction
endclass
module t;
  c o;
  int tmp[];
  initial begin
    automatic int l[][];
    o = new();
    o.build();
    $display("cls %0d %0d %0d | %0d %0d %0d | %0d %0d", o.v[0].size(), o.v[0][0], o.v[0][1],
             o.v[1].size(), o.v[1][0], o.v[1][2], o.v[2].size(), o.v[2][2]);
    tmp = '{3, 4};
    o.v[1] = tmp;
    $display("hdl %0d %0d %0d", o.v[1].size(), o.v[1][0], o.v[1][1]);
    l = new[2];
    l[1] = tmp;
    $display("loc %0d %0d %0d %0d", l[0].size(), l[1].size(), l[1][0], l[1][1]);
  end
endmodule
"#,
        100,
    )
    .unwrap();
    let lines: Vec<_> = result
        .output
        .iter()
        .map(|entry| entry.message.as_str())
        .collect();
    assert_eq!(lines, ["cls 2 8 9 | 3 6 8 | 3 8", "hdl 2 3 4", "loc 0 2 3 4"]);
}
