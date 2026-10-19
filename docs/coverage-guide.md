# Coverage in xezim

xezim collects **functional coverage** (covergroups, IEEE 1800 clause 19) and
**assertion coverage** (`cover` statements, plus pass/fail counts of assertions,
clause 16) in every simulation, and writes the results to a JSON file when the run
ends. When asked (`--code-coverage`), it also collects **code coverage**:
statement, branch, condition, expression, toggle and FSM (state machine)
counts, in the same file.

This guide covers what is supported, how to write coverage that xezim collects, and
how to read the results. Code coverage has [its own section](#code-coverage).

---

## What's supported

| Area | Supported |
|---|---|
| Where covergroups live | Any module (the top or an instantiated one), an interface, a package, file scope, and classes. See [Where a covergroup can be declared](#where-a-covergroup-can-be-declared) |
| Sampling | A sampling event (`covergroup cg @(posedge clk)`, `@(posedge clk iff en)`, a named event `@(ev)`), explicit `sample()`, `with function sample(...)` arguments, constructor arguments (`covergroup cg (int lo, int hi)`) |
| Bins | Values and ranges (`{0, [2:5], [8:$]}`), automatic bins, array bins `name[]` and `name[N]`, `wildcard` bins, transition bins (`(1 => 2 => 3)`, `([0:1] => [2:3])`, sets `(1, 5 => 3)`, lists `(1 => 2), (5 => 6)`, repetition `[*n]`, `[->n]`, `[=n]`), `default`, `ignore_bins`, `illegal_bins` |
| Guards | `coverpoint x iff (cond)`, `cross a, b iff (cond)` |
| Crosses | Automatic cross bins (every combination of the coverpoints' bins); `bins`, `ignore_bins` and `illegal_bins` selected with `binsof(cp)`, `binsof(cp.bin)`, `intersect {...}`, `!`, `&&`, `\|\|` and `with (...)` |
| Options | `option.at_least`, `option.weight`, `option.auto_bin_max`, `type_option.merge_instances`, `type_option.weight` |
| Queries | `get_inst_coverage()` and `get_coverage()` on a covergroup, a coverpoint or a cross, with or without `(covered, total)`; `cg_type::get_coverage()`; `$get_coverage()`; `start()` and `stop()` |
| Assertion coverage | Counts for `cover property`, `cover sequence`, `assert property`, `assume property` and the immediate `cover`, `assert` and `assume` |
| Code coverage | Statement, branch, condition, expression, toggle and FSM coverage with `--code-coverage` (or `+cover`). See [Code coverage](#code-coverage) |
| Results | The `xezim_cov.json` file, and `[COV]` summary lines with `--verbose` |

---

## Enabling coverage

Nothing needs enabling. Covergroups and assertions are always compiled and
evaluated, and a run that has at least one covergroup instance or assertion writes
`xezim_cov.json` to the current directory when it ends:

```bash
xezim alu_cov.sv          # also writes ./xezim_cov.json
```

- `XEZIM_COV_DB=<path>` writes the file somewhere else. `XEZIM_COV_DB=/dev/null`
  skips it.
- `+fcover` is accepted and does nothing.
- Code coverage is off unless asked for: see
  [Enabling code coverage](#enabling-code-coverage). `+cover`, `+cover=<spec>`
  and `-coverage` turn it on.

---

## Writing coverage

Give every coverpoint and cross a label (`cp_op : coverpoint op`). Queries and the
results file use the label as the name. An unlabeled coverpoint on a variable is
named after the variable (`coverpoint data` is `cg.data`); one on any other
expression gets the name `coverpoint#<n>`, `<n>` counting the covergroup's
coverpoints from 1, which a query cannot spell.

### A covergroup in a module, sampled on a clock

```systemverilog
module tb;
  logic       clk = 0;
  logic       valid;
  logic [1:0] op;
  logic [3:0] a;

  always #5 clk = ~clk;

  // Sampled on every rising clock edge.
  covergroup alu_cg @(posedge clk);
    cp_op : coverpoint op iff (valid) {   // skipped while valid is 0
      bins add       = {0};
      bins sub       = {1};
      bins logic_ops = {[2:3]};
    }
    cp_a : coverpoint a {
      bins zero = {0};
      bins low  = {[1:7]};
      bins high = {[8:15]};
    }
  endgroup

  alu_cg cg = new();

  initial begin
    valid = 1;
    op = 0; a = 0; @(negedge clk);
    op = 1; a = 3; @(negedge clk);
    op = 0; a = 5; @(negedge clk);
    valid = 0; op = 3; a = 9; @(negedge clk);   // op not sampled
    $display("cp_op  = %0.2f%%", cg.cp_op.get_inst_coverage());
    $display("cp_a   = %0.2f%%", cg.cp_a.get_inst_coverage());
    $display("alu_cg = %0.2f%%", cg.get_inst_coverage());
    $finish;
  end
endmodule
```

```text
$ xezim alu_cov.sv
cp_op  = 66.67%
cp_a   = 100.00%
alu_cg = 83.33%
Simulation finished at time 40 ($finish called)
```

`logic_ops` was never hit: the only `op = 3` came while `valid` was 0. So `cp_op` has
2 of its 3 bins, and the group is the average of its two coverpoints.

To print coverage when the run ends, however it ends (`$finish`, `$fatal` or
`--max-time`), put the queries in a `final` block:

```systemverilog
final $display("alu_cg: %0.2f%%", cg.get_inst_coverage());
```

### A covergroup in a class, sampled by `sample()`

A covergroup declared in a class is created in the class constructor. Its
coverpoints read the object's properties.

```systemverilog
class packet;
  rand bit [3:0] len;
  rand bit [1:0] kind;

  covergroup pkt_cg;
    cp_len : coverpoint len {
      bins short_p = {[0:3]};
      bins mid_p   = {[4:11]};
      bins long_p  = {[12:15]};
    }
    cp_kind : coverpoint kind;   // automatic bins: one per value, 4 bins
  endgroup

  function new();
    pkt_cg = new();              // a class covergroup is created in new()
  endfunction
endclass

module tb;
  initial begin
    packet p = new();
    void'(p.randomize() with { len == 2;  kind == 0; });  p.pkt_cg.sample();
    void'(p.randomize() with { len == 13; kind == 1; });  p.pkt_cg.sample();
    $display("cp_len  : %0.2f%%", p.pkt_cg.cp_len.get_inst_coverage());
    $display("cp_kind : %0.2f%%", p.pkt_cg.cp_kind.get_inst_coverage());
    $display("pkt_cg  : %0.2f%%", p.pkt_cg.get_inst_coverage());
    $display("overall : %0.2f%%", $get_coverage());
  end
endmodule
```

```text
$ xezim pkt_cov.sv
cp_len  : 66.67%
cp_kind : 50.00%
pkt_cg  : 58.33%
overall : 58.33%
Simulation finished at time 0
```

To pass the values in at the sampling call instead, declare the covergroup with a
sample function. This is the usual form for a monitor or subscriber:

```systemverilog
covergroup txn_cg with function sample (bit [3:0] addr, bit wr);
  cp_addr : coverpoint addr { bins lo = {[0:7]}; bins hi = {[8:15]}; }
  cp_wr   : coverpoint wr;
endgroup
...
txn_cg.sample(addr, wr);
```

### Crosses and `ignore_bins`

```systemverilog
module tb;
  bit [1:0] mode;
  bit [1:0] size;
  bit       en;

  covergroup cfg_cg;
    cp_mode : coverpoint mode {
      bins m[] = {[0:3]};            // one bin per value: m[0] .. m[3]
      ignore_bins reserved = {3};    // mode 3 is never used: drop m[3]
    }
    cp_size : coverpoint size {
      bins narrow = {0, 1};
      bins wide   = {2, 3};
    }
    mode_x_size : cross cp_mode, cp_size iff (en);   // 3 x 2 = 6 bins
  endgroup

  cfg_cg cg = new();

  initial begin
    en = 1;
    mode = 0; size = 0; cg.sample();
    mode = 1; size = 2; cg.sample();
    mode = 2; size = 3; cg.sample();
    mode = 3; size = 0; cg.sample();   // ignored mode: no cross bin
    en = 0;
    mode = 0; size = 3; cg.sample();   // cross not sampled (iff)
    $display("cp_mode     %0.2f", cg.cp_mode.get_inst_coverage());
    $display("cp_size     %0.2f", cg.cp_size.get_inst_coverage());
    $display("mode_x_size %0.2f", cg.mode_x_size.get_inst_coverage());
    $display("cfg_cg      %0.2f", cg.get_inst_coverage());
  end
endmodule
```

```text
$ xezim cross_cov.sv
cp_mode     100.00
cp_size     100.00
mode_x_size 50.00
cfg_cg      83.33
Simulation finished at time 0
```

The ignored value takes `m[3]` out of `cp_mode`, so the cross has 3 x 2 = 6 bins.
Three of them are hit: `(m[0], narrow)`, `(m[1], wide)` and `(m[2], wide)`. The last
sample is skipped by the cross's `iff` but still counts for the coverpoints.

To cover chosen parts of a cross instead of every combination, give the cross its
own bins. Coverage is then the fraction of those bins that were hit:

```systemverilog
axb : cross cp_a, cp_b {
  bins a_lo = binsof(cp_a) intersect {[0:1]};
  bins a_hi = binsof(cp_a) intersect {[2:3]};
}
```

### How coverage is computed

- **Explicit bins:** a coverpoint's coverage is the number of its `bins` that were hit
  divided by the number of `bins`. `ignore_bins` and `illegal_bins` values are removed
  from the other bins (a sampled ignored value hits no bin), and a bin left with no
  values is dropped. A `default` bin is not counted.
- **Automatic bins** (a coverpoint with no `bins`): one bin per value of the sampled
  expression's width, or `option.auto_bin_max` (default 64) equal ranges when there
  are more values than that. A 4-bit coverpoint has 16 bins; an `int` has 64.
- **Array bins** `name[]`: one bin per value, named `name[<value>]`. `name[N]`: N bins
  `name[0]` to `name[N-1]`; the listed values, in order, are split evenly among them
  and the last one also takes the remainder.
- **Transition bins** `(1 => 2 => 3)`: hit when consecutive samples of the coverpoint
  match the sequence. Each step is a value set (values and ranges, `1, [3:4]`) with
  an optional repetition: `v [*n]` (n consecutive samples), `v [->n]` (n samples,
  not necessarily consecutive, ending on the last), `v [=n]` (the same, followed by
  any samples of other values), each also as `n:m`. A bin with a list
  `(1 => 2), (5 => 6)` is hit by any of them; an array bin `name[] = (...)` has one
  bin per value sequence.
- **Crosses:** every combination of the crossed coverpoints' bins. A crossed variable
  that has no coverpoint of its own gets automatic bins. A `bins` of the cross body
  is one bin holding the combinations its select expression picks; an `ignore_bins`
  or `illegal_bins` removes the combinations it picks; every other combination
  stays an automatic bin of its own. `binsof(cp) intersect {v}` picks the
  combinations whose `cp` bin holds a value in `{v}`, and `with (expr)` those with a
  value tuple for which `expr` holds.
- **Covergroup:** the average of its coverpoints and crosses, weighted by each
  one's `option.weight` (default 1).

A hit on an `illegal_bins` value or transition is a run-time error: xezim prints
`** Error: Illegal bin hit at value 7: cg.cp_s.bad` with the time and scope, like
`$error`, and `--error-exit` counts it. The run carries on.

### Options and multiple instances

```systemverilog
covergroup opt_cg;
  option.at_least = 2;               // a bin needs 2 hits to count
  cp_x : coverpoint x { bins b0 = {0}; bins b1 = {1}; }
  cp_y : coverpoint y {
    option.weight   = 3;             // counts 3x in the group average
    option.at_least = 1;             // overrides the group setting
    bins b0 = {0}; bins b1 = {1};
  }
endgroup
```

After samples `(x, y)` = `(0, 0)`, `(0, 1)`, `(1, 1)`, `cp_x` has 50.00 (`b1` was hit
only once), `cp_y` 100.00, and `opt_cg` (50 + 3 x 100) / 4 = 87.50.

| Option | Where | Effect |
|---|---|---|
| `option.at_least = N` | covergroup, coverpoint or cross | A bin (explicit, automatic or cross) counts as covered once it has N hits (default 1). A coverpoint's or cross's setting overrides the group's |
| `option.weight = N` | coverpoint or cross | Weight of the coverpoint or cross in its covergroup's average (default 1) |
| `option.auto_bin_max = N` | covergroup or coverpoint | Maximum number of automatic bins (default 64) |
| `type_option.merge_instances = 1` | covergroup, or `cg_type::type_option.merge_instances = 1;` at run time | `get_coverage()` adds up the instances' hits: a bin is covered once their total reaches `at_least`, and `get_inst_coverage()` returns the same number. Default 0: the average of the instances' coverages |
| `type_option.weight = N` | covergroup, or `cg_type::type_option.weight = N;` | Weight of the covergroup type in `$get_coverage()` (default 1) |

Every `new()` creates a separate instance with its own hits. `get_inst_coverage()`
reports one instance; `get_coverage()` reports the covergroup type:

```systemverilog
covergroup val_cg with function sample (bit [3:0] v);
  cp : coverpoint v { bins lo = {[0:1]}; bins hi = {[14:15]}; }
endgroup

val_cg a = new();
val_cg b = new();

initial begin
  a.sample(0);    // a hits lo
  b.sample(15);   // b hits hi
  $display("a %0.2f  b %0.2f  type %0.2f", a.get_inst_coverage(), b.get_inst_coverage(), a.get_coverage());
  val_cg::type_option.merge_instances = 1;
  $display("merged type %0.2f", a.get_coverage());
end
```

```text
a 50.00  b 50.00  type 50.00
merged type 100.00
```

### Assertion coverage

Every `cover`, `assert` and `assume` is counted, with no extra code. A
`cover property` action block runs on each match:

```systemverilog
module tb;
  logic clk = 0, rst = 1, req = 0, gnt = 0;
  always #5 clk = ~clk;

  // Concurrent cover: a request answered by a grant on the next clock.
  c_req_gnt : cover property (@(posedge clk) disable iff (rst) req ##1 gnt)
    $display("%0t: request granted", $time);

  // Concurrent assert: never a grant without a request on the clock before.
  a_gnt_has_req : assert property (@(posedge clk) disable iff (rst) gnt |-> $past(req))
    else $error("grant without request");

  // Immediate cover, evaluated on every clock.
  always @(posedge clk) c_waiting : cover (req && !gnt);

  initial begin
    @(negedge clk) rst = 0;
    @(negedge clk) req = 1;
    @(negedge clk) begin req = 0; gnt = 1; end
    @(negedge clk) gnt = 0;
    @(negedge clk) req = 1;
    @(negedge clk) begin req = 0; gnt = 1; end
    @(negedge clk) gnt = 0;
    $finish;
  end
endmodule
```

```text
$ xezim handshake.sv
35: request granted
65: request granted
Simulation finished at time 70 ($finish called)
```

The counts are in the results file; see [Assertion entries](#assertion-entries).

---

## Collecting and reading results

### The results file

When the run ends, by `$finish`, `$fatal`, `--max-time` or running out of events,
xezim writes `xezim_cov.json` to the current directory, replacing any earlier one.

- It is written only when the design has at least one covergroup instance or one
  assertion that was evaluated, or when the run collects code coverage. A run
  with none of them leaves an old `xezim_cov.json` as it was.
- `XEZIM_COV_DB=<path>` writes it to `<path>` instead. The directory must exist.
- Other simulators' `-covworkdir <dir>`, `-covscope <name>` and `-covtest <name>`
  write it to `<dir>/<scope>/<test>/xezim_cov.json` (defaults `cov_work`,
  `scope`, `test`), creating the directory. `XEZIM_COV_DB` wins over them.
- When the file can't be written, xezim prints
  `[COV] warning: could not write <path>: <reason>` and the run's result is
  unchanged.

The file for the [module example](#a-covergroup-in-a-module-sampled-on-a-clock):

```json
{
  "sim_time": 40,
  "assertion_sites": 0,
  "assertion_pass_total": 0,
  "assertion_fail_total": 0,
  "assertions": [
  ],
  "covergroups": [
    {"name": "alu_cg", "samples": 4, "coverpoints": {"cp_op": 2, "cp_a": 4}, "crosses": {}, "bins": {"cp_a.zero": 1, "cp_op.add": 2, "cp_op.sub": 1, "cp_a.low": 2, "cp_a.high": 1}}
  ]
}
```

| Field | Meaning |
|---|---|
| `sim_time` | The time the run ended: the same number as the closing `Simulation finished at time N` line |
| `assertion_sites` | Number of entries in `assertions` |
| `assertion_pass_total`, `assertion_fail_total` | The sums of the `pass` and `fail` fields of `assertions` |
| `assertions` | One entry per assertion or cover statement; see below |
| `covergroups` | One entry per covergroup instance, in creation order |
| `covergroups[].name` | The covergroup type: `alu_cg`, `packet::pkt_cg` for one declared in class `packet`, or `u0.v_cg` for one declared in the module instantiated as `u0` (the instance path below the top module). Instances of one type share the name |
| `covergroups[].samples` | How many times the instance was sampled (sampling events plus `sample()` calls) |
| `covergroups[].coverpoints` | Per coverpoint, the number of **distinct values** sampled (not bins) |
| `covergroups[].crosses` | Per cross, the number of distinct value combinations sampled |
| `covergroups[].bins` | Hit count per explicit bin, keyed `<coverpoint>.<bin>`, `<coverpoint>.<bin>[<value>]` for `name[]` array bins and `<coverpoint>.<bin>[<index>]` for `name[N]`, and per bin of a cross body, keyed `<cross>.<bin>`. Includes `default` and `illegal_bins` bins |
| `code_coverage` | Only in a run that collects code coverage; see [The code coverage results](#the-code-coverage-results) |

Things the file does not hold for functional coverage:

- **Percentages.** Print them from the testbench with the
  [query functions](#query-functions), for example in a `final` block.
- **Bins that were never hit.** `alu_cg` above has no `cp_op.logic_ops` entry.
  Compare against the covergroup's source to find the holes.
- Automatic bins, of a coverpoint or a cross.

Key order in the objects can change from run to run. Compare files with
`jq -S . xezim_cov.json`, not with a plain `diff`.

#### Assertion entries

For the [assertion example](#assertion-coverage):

```json
  "assertions": [
    {"file": "handshake.sv", "line": 6, "span_start": 95, "kind": "cover", "pass": 2, "fail": 0},
    {"file": "handshake.sv", "line": 10, "span_start": 222, "kind": "assert", "pass": 6, "fail": 0},
    {"file": "handshake.sv", "line": 14, "span_start": 372, "kind": "cover", "pass": 2, "fail": 5}
  ],
```

| Statement | `pass` | `fail` |
|---|---|---|
| `cover property`, `cover sequence` | Matches | Always 0 |
| `assert property`, `assume property` | Attempts that succeeded, including vacuous ones (`gnt` low, so `gnt \|-> ...` holds) | Attempts that failed |
| Immediate `cover`, `assert`, `assume` | Evaluations where the expression was true | Evaluations where it was false |

An immediate `cover`'s false evaluations therefore count in `assertion_fail_total`
too. In the example, all 5 fails are clock edges where `req && !gnt` was false; no
assertion failed.

`file` and `line` locate the `cover` / `assert` / `assume` statement; `span_start`
is its position in that file after preprocessing. The entries are sorted by file
and position. An entry has no name.

All instances of a module share one entry: its counts are the sum over the
instances.

### The summary lines

`--verbose` (or `XEZIM_VERBOSE=1`) adds `[COV]` lines at the end of the run, on
stderr, together with the other engine lines:

```text
$ xezim --verbose alu_cov.sv 2>&1 | grep '^\[COV\]'
[COV] assertions: 0 sites (assert=0, assume=0, cover=0) — 0 passes, 0 fails
[COV] coverage: 1 covergroup instances, 4 samples, 6 unique coverpoint values, 0 unique cross tuples
[COV] wrote coverage DB to xezim_cov.json
```

The counts are totals over the whole design, with the same meanings as the file's
fields: "unique coverpoint values" adds up the file's `coverpoints` numbers (2 + 4).

### Query functions

All return a `real` percentage from 0.0 to 100.0; print it with `%f`, for example
`%0.2f`.

| Call | Returns |
|---|---|
| `cg.get_inst_coverage()` | Coverage of the instance `cg` |
| `cg.get_coverage()` | Coverage of the covergroup type: the average over its instances, or of their hits added up with `type_option.merge_instances = 1` |
| `cg.<cp>.get_inst_coverage()`, `cg.<cp>.get_coverage()` | The same for one coverpoint or cross `<cp>` |
| `cg_type::get_coverage()`, `cg_type::<cp>::get_coverage()` | The same as `get_coverage()` on an instance of type `cg_type` |
| `$get_coverage()` | The average over every covergroup type, weighted by `type_option.weight` |
| `cg.sample()`, `cg.sample(args)` | Samples the instance now |
| `cg.stop()`, `cg.start()`, `cg.<cp>.stop()`, `cg.<cp>.start()` | Stop sampling the instance (or one coverpoint or cross of it), and start it again |

Each coverage query also takes two `int` output arguments,
`get_inst_coverage(covered, total)`: they receive the number of covered bins and the
total number of bins, summed over the coverpoints and crosses for a covergroup.
`get_coverage` sums them over the instances, or counts the bins of their added-up
hits with `type_option.merge_instances = 1`. With `merge_instances`,
`get_inst_coverage()` returns the type's numbers too.

### Several runs

There is no merge tool: every run writes its own file. Give each run its own path,
for example one per seed:

```systemverilog
module tb;
  bit [3:0] len;
  covergroup len_cg;
    cp_len : coverpoint len {
      bins short_p = {[0:3]};
      bins mid_p   = {[4:11]};
      bins long_p  = {[12:15]};
    }
  endgroup
  len_cg cg = new();
  initial begin
    repeat (2) begin
      len = $urandom_range(0, 15);
      cg.sample();
    end
    $display("len_cg = %0.2f%%", cg.get_inst_coverage());
  end
endmodule
```

```text
$ mkdir -p cov
$ for s in 1 2 3; do XEZIM_COV_DB=cov/seed$s.json xezim +seed=$s len_cov.sv; done
len_cg = 66.67%
Simulation finished at time 0
len_cg = 66.67%
Simulation finished at time 0
len_cg = 33.33%
Simulation finished at time 0
```

The files hold hit counts per bin, so the bins hit across all runs can be added up.
For example, with `jq`:

```text
$ jq -r '.covergroups[] | .name as $cg | .bins | to_entries[] | "\($cg).\(.key) \(.value)"' cov/seed*.json \
    | awk '{h[$1] += $2} END {for (b in h) print b, h[b]}' | sort
len_cg.cp_len.long_p 2
len_cg.cp_len.mid_p 4
```

`short_p` was not hit by any seed. The files don't record the bins that were never
hit, so a combined percentage can't be worked out from them alone.

---

## Code coverage

With `--code-coverage`, xezim also counts which statements ran, which way each
branch went, which terms of each condition and logic expression decided its
value, which bits of each signal toggled, and which states and transitions
of each state machine the run visited, and writes the counts to
`xezim_cov.json` next to the functional coverage.

### Enabling code coverage

| Spelling | Collects |
|---|---|
| `--code-coverage` | Statement, branch, condition, expression, toggle and FSM coverage |
| `--code-coverage=<kinds>` | The kinds listed, separated by commas: `stmt` (or `statement`), `branch`, `cond` (or `condition`), `expr` (or `expression`), `toggle`, `fsm`, `all` |
| `XEZIM_CODE_COVERAGE=<kinds>` | The same, for scripts that cannot add a flag. `--code-coverage` wins |
| `+cover` | All six |
| `+cover=<letters>` | `s` statement, `b` branch, `c` condition, `e` expression, `t` toggle, `f` FSM. Other simulators' other letter (`x` extended toggle) is ignored with one warning, for example `Warning: +cover=sbx: xezim has no extended toggle (x) coverage; collecting statement, branch coverage` |
| `-coverage` | All six, unless a `+cover=<letters>` names fewer |
| `-coverage <types>` | `all` (or `A`): all six. Otherwise types joined by `:`, in either case: `B` block (statement and branch), `E` expression (condition and expression), `T` toggle, `F` FSM, `U` functional (always collected). A `+cover=<letters>` wins |

`--code-coverage-scope=<path>[,<path>...]` (repeatable) limits every kind to the
instances at or below the given paths (`tb.dut`, or `dut` for the instance of that
name below the top) and to the packages named. Without it the whole design is
covered, except the UVM library package `uvm_pkg` and the built-in `std` package;
name one of them in the scope to include it.

`--no-toggle-input-ports` (or `XEZIM_TOGGLE_INPUT_PORTS=0`) leaves input and
inout ports out of toggle coverage, so that only the signal driving a port
counts its toggles. By default each instance counts its own ports; see
[Toggles](#toggles). `--toggle-input-ports` restores the default when the
variable is set.

Code that a constant condition rules out in an instance is left out of the
totals, as in other simulators: see [Constant conditions](#constant-conditions).
`--code-coverage-constant=keep` (or `XEZIM_CODE_COVERAGE_CONSTANT=keep`) counts
it like any other code; `--code-coverage-constant=exclude` is the default, and
the flag wins over the variable.

Code coverage is off by default, and then costs nothing: the design compiles and
runs exactly as it does without the feature. When it is on, statements,
branches, conditions and expressions are instrumented as the design is
compiled, before xezim lowers it to bytecode, so every execution path (compiled
blocks, the two-state executors and the interpreter) counts the same way, and
the simulation's results do not change. The instrumented run is slower, most of
all for RTL:

| Run | Without | `=stmt,branch,toggle` | `=stmt,branch` | `=toggle` |
|---|---|---|---|---|
| C906 RISC-V CPU running one CoreMark iteration (RTL) | 112 s | 298 s; 2.36x the instructions | 2.17x the instructions | 1.23x the instructions |
| AXI4 UVM testbench (`axi4_base_test`) | 7.0 s | 7.2 s; +0.1% instructions | - | - |

The C906 run's results file is 23 MB: 47,632 statements, 23,603 branch arms and
979,190 toggle bins in 1,773 scopes.

Condition and expression coverage cost more per item: the interpreter evaluates
each covered condition and expression, term by term, where the rest of the
design runs compiled. A UART with its testbench (135 condition and expression
terms) runs 1.55x the instructions of a run without coverage with
`=cond,expr`, against 1.22x with `=stmt,branch,toggle` and 1.73x with all
five.

FSM coverage instruments only the always blocks that hold a state register. On
CVDP designs with state machines, `--code-coverage=fsm` took 2 to 3% more CPU
time than a run without code coverage for `helmholtz` (six FSMs) and
`search_algorithm`, and 21 to 26% more for `uart`, whose state register lives
in one large block with 40 inputs.

### An example

```systemverilog
module counter (input logic clk, rst, en, output logic [2:0] q);
  always_ff @(posedge clk)
    if (rst)
      q <= 0;
    else if (en && q != 3'd7)
      q <= q + 1;
endmodule

module tb;
  logic clk = 0, rst = 1, en = 0;
  logic [2:0] q;
  counter u_cnt (.clk, .rst, .en, .q);
  always #5 clk = ~clk;
  initial begin
    @(negedge clk) rst = 0;
    en = 1;
    repeat (3) @(negedge clk);
    $display("q = %0d", q);
    $finish;
  end
endmodule
```

```text
$ xezim --code-coverage cnt_cov.sv
q = 3
Simulation finished at time 40 ($finish called)
$ xezim --code-coverage --verbose cnt_cov.sv 2>&1 | grep '^\[COV\] code\|^\[COV\]  '
[COV] code coverage: statement 12/12 (100.00%), branch 2/3 (66.67%), toggle 14/24 (58.33%), condition 0/2 (0.00%), expression 0/0, fsm 0/0
[COV]   tb (tb): statement 9/9 (100.00%), branch 0/0, toggle 7/12 (58.33%), condition 0/0, expression 0/0, fsm 0/0
[COV]   tb.u_cnt (counter): statement 3/3 (100.00%), branch 2/3 (66.67%), toggle 7/12 (58.33%), condition 0/2 (0.00%), expression 0/0, fsm 0/0
```

The first line is the design's totals, then one line per scope with its design
unit in parentheses (at most 50 scopes; the file has them all).

The counter's entry in `xezim_cov.json`:

```json
      {
        "scope": "tb.u_cnt",
        "design_unit": "counter",
        "kind": "instance",
        "statement": {"covered": 3, "total": 3, "percent": 100.00},
        "branch": {"covered": 2, "total": 3, "percent": 66.67},
        "toggle": {"covered": 7, "total": 12, "percent": 58.33},
        "condition": {"covered": 0, "total": 2, "percent": 0.00},
        "expression": {"covered": 0, "total": 0, "percent": null},
        "fsm": {"covered": 0, "total": 0, "percent": null, "states": {"covered": 0, "total": 0, "percent": null}, "transitions": {"covered": 0, "total": 0, "percent": null}},
        "statements": [{"file": "cnt_cov.sv", "line": 2, "kind": "always_ff", "count": 4}, {"file": "cnt_cov.sv", "line": 4, "kind": "statement", "count": 1}, {"file": "cnt_cov.sv", "line": 6, "kind": "statement", "count": 3}],
        "branches": [{"file": "cnt_cov.sv", "line": 3, "kind": "if", "arms": [{"line": 4, "arm": "if", "count": 1}, {"line": 6, "arm": "else if", "count": 3}, {"line": 3, "arm": "else (implicit)", "count": 0}]}],
        "toggles": [{"signal": "clk", "width": 1, "type": "logic", "rise": [4], "fall": [4]}, {"signal": "en", "width": 1, "type": "logic", "rise": [1], "fall": [0]}, {"signal": "q", "width": 3, "type": "logic", "rise": [2, 1, 0], "fall": [1, 0, 0]}, {"signal": "rst", "width": 1, "type": "logic", "rise": [0], "fall": [1]}],
        "conditions": [{"file": "cnt_cov.sv", "line": 5, "kind": "if", "text": "en && q != 3'd7", "terms": [{"term": "en", "hits": [0, 3]}, {"term": "q != 3'd7", "hits": [0, 3]}]}],
        "expressions": [],
        "fsms": []
      }
```

The flop ran on four clock edges: once in reset, three times counting. No edge
came with both `rst` and `en` low, so the implicit `else` of the `if` chain was
never taken. `q` went 0, 1, 2, 3: bit 2 never toggled and bit 1 never fell, so
3 of its 6 bins are covered. The inputs count in the counter too, with the
testbench signals' counts: `clk` rose and fell four times, `rst` only fell and
`en` only rose, so 7 of the counter's 12 toggle bins are covered. The `else if`
condition was evaluated on the three counting edges, each time with `en` at 1
and `q != 3'd7` true: both terms decided it at 1 (their row 1), but neither
was ever seen deciding it at 0, so neither term is covered.
`en` only rose, so 7 of the counter's 12 toggle bins are covered. `q` is no
state machine: nothing tests it (see [FSMs](#fsms)).

### What counts

#### Statements

- Every procedural statement of `initial`, `always`, `always_comb`, `always_ff`,
  `always_latch` and `final` blocks, tasks, functions and class methods counts
  each time it starts: assignments, calls, system task calls, `return`,
  `break`, `continue`, `disable`, `->`, `wait fork`, immediate assertions, and
  procedural `assign`, `deassign`, `force` and `release`.
- The `always` construct is a statement of its own, of kind `always`,
  `always_comb`, `always_ff` or `always_latch`: it counts each time its body
  starts, once per event for `always @(...)` and once per evaluation for
  `always_comb`. An `always` that starts with a delay (`always #5 clk = ~clk;`)
  counts when the delay has passed.
- A timing control and the statement it guards are two statements:
  `#1 a = 1;` counts the delay (kind `delay`) when it starts waiting and the
  assignment when it runs. `@(...)` is kind `event`, `wait (...)` kind `wait`.
- A loop (`for`, `foreach`, `while`, `do`, `repeat`, `forever`) counts once each
  time it starts; the statements of its body count once per iteration.
- `if` and `case` are not statements (they are [branches](#branches)), and
  neither are `begin`/`end`, `fork`/`join` and declarations. The statements
  inside them count.
- Each continuous assignment is a statement of kind `assign`: an `assign`
  (one per target when it lists several) and a net declaration assignment
  (`wire w = a & b;`). It counts each time xezim evaluates it.
- Not counted: the initializers of variables declared outside procedural code,
  port connections and gate primitives.

#### Branches

- `if`: one arm per condition of an `if` / `else if` chain, and one for the
  final `else`. A chain without a final `else` gets an `else (implicit)` arm
  that counts the times no condition held. A `unique`, `unique0` or `priority`
  `if` has no implicit arm, since no condition holding is a violation.
- `case`, `casez`, `casex` and `case inside`: one arm per case item, the
  `default` item included. An item listing several expressions (`0, 1: ...`) is
  one arm. A `case` without a `default` gets a `default (implicit)` arm, except a
  `unique`, `unique0` or `priority` one.
- `randcase`: one arm per item.
- `?:`: a `true` and a `false` arm, counted each time the expression is
  evaluated, in the value of an assignment (procedural or continuous) or a
  `return`, in the arguments of a task or function call, and in an `if`
  condition. A condition with x or z bits counts for neither.
- An arm is covered once its count is at least 1; branch coverage is the
  covered arms over all arms.

#### Constant conditions

A parameter's value is fixed when the design is elaborated (IEEE 1800 §11.2.1),
so in each instance a condition built from parameters always goes the same way,
and the code it rules out can never run there. As in other simulators, that code
is left out of the instance's totals (*excluded*):

- `if` / `else if`: an arm whose condition is constantly false is excluded, and
  so is every arm after one whose condition is constantly true, the implicit
  `else` included. When every condition of the chain is a constant, the chain is
  no decision at all: the whole branch is excluded, and only the statements of
  the arm it takes count.
- `case`, `casez` and `casex` whose selector and every item expression are
  constants: the whole branch is excluded, and so is every item but the one
  that matches (the `default`, written or implicit, when none does). Wildcard
  items match as they do at run time. A `case` with any item that is not a
  constant, such as `case (1'b1)` over signals, keeps all of its arms, and so
  does a `case inside`, as in other simulators.
- `?:` whose condition is a constant: the branch is excluded, and so is any
  `?:` in the operand it never yields, or in the right operand of an `&&`
  whose left operand is constantly false (an `||`, constantly true), which
  is never evaluated.
- The statements of an excluded arm are excluded, and so are the branches
  inside it, and the `?:`s in the conditions of the `else if`s after a
  constantly true one.
- The [condition and expression](#conditions-and-expressions) items of that
  code (an excluded arm, an operand that is never evaluated, the condition of
  an `else if` after a constantly true one) are excluded too. They are left
  out of the results altogether.

Constants are literals (strings included), parameters and `localparam`s, enum
constants, package parameters (`pkg::P`), the genvar of a generate loop, and an
input port that the instantiation ties to a literal (`.en(1'b1)`), combined with
operators, `?:`, concatenation, replication, bit and part selects of a
parameter, and `$bits`, `$clog2`, `$signed`, `$unsigned`, `$countones`,
`$onehot` and `$onehot0`. An `&&` with a constantly false operand, or an `&`
with an all-zero one, is constantly false whatever its other operand is; an
`||` with a constantly true operand, or an `|` with a nonzero one, is constantly
true. Each constant is evaluated once the design is compiled, by the same
evaluator as the run, so an excluded statement or arm is one the run can never
reach. A condition with x or z bits and no 1 bit is not decided. The methods of
classes and the subroutines of packages are never excluded.

Generate constructs (`if`, `case` and `for` generate) are decided by
elaboration: the blocks a generate condition leaves out are not part of the
design, so they have no entries at all.

For example, a reduction module that one instance uses as an OR:

```systemverilog
module reduce #(parameter [1:0] OP = 0) (input logic [3:0] a, output logic y);
  always_comb
    case (OP)
      0: y = &a;
      1: y = |a;
      default: y = ^a;
    endcase
endmodule
```

`reduce #(.OP(1)) u_or` counts the `always_comb` and `y = |a;`; the other two
statements and the `case`, a constant decision, are excluded:

```json
        "statement": {"covered": 2, "total": 2, "percent": 100.00, "excluded": 2},
        "branch": {"covered": 0, "total": 0, "percent": null, "excluded": 3},
        "statements": [{"file": "red.sv", "line": 3, "kind": "always_comb", "count": 3}, {"file": "red.sv", "line": 4, "kind": "statement", "count": 0, "excluded": "constant"}, {"file": "red.sv", "line": 5, "kind": "statement", "count": 3}, {"file": "red.sv", "line": 6, "kind": "statement", "count": 0, "excluded": "constant"}],
        "branches": [{"file": "red.sv", "line": 3, "kind": "case", "excluded": "constant", "arms": [{"line": 4, "arm": "item", "count": 0, "excluded": "constant"}, {"line": 5, "arm": "item", "count": 3}, {"line": 6, "arm": "default", "count": 0, "excluded": "constant"}]}]
```

With `--code-coverage-constant=keep` the same instance has statement 2/4 and
branch 1/3, and no `excluded` members.

#### Toggles

- Each bit has two bins: a rise (0 to 1) and a fall (1 to 0). Transitions to or
  from x or z do not count.
- A bit is compared with its value at the end of the previous time slot, the way
  a waveform dump sees it: a signal that goes 0, 1, 0 within one time slot, even
  across delta cycles, does not toggle. The first comparison is with the value
  the signal starts with.
- Tracked: every net and variable of an integral type declared in a module or
  interface instance: `logic`, `bit` and `reg` vectors, nets, `int`, `integer`,
  `byte`, `shortint`, `longint`, enums and packed structs (both as plain bit
  vectors), and the members of unpacked structs.
- Ports are tracked in the instance they belong to, input, output and inout
  alike (the ports of an interface instance included), with the value their
  connection gives them. A port connected straight to a parent's signal shows
  the same counts as that signal, under both names. A port connected to an
  expression, a select or a concatenation toggles as that value does; the
  bits a narrower connection leaves at 0 never toggle, and neither does a port
  tied to a constant or left unconnected, so their bins stay uncovered.
  `--no-toggle-input-ports` leaves input and inout ports out, so that only the
  signal driving one counts its toggles.
- Not tracked: unpacked arrays (unpacked port arrays included), queues, dynamic
  and associative arrays, `real`, `time`, `string`, `event` and class-handle
  variables, parameters, interface ports and modports (the interface instance
  counts its signals), variables declared in tasks, functions and procedural
  blocks, and class properties.
- Also left out: a variable whose value is fixed before the run starts, so it
  has no toggle to count (§6.8). That is one only its declaration initializer
  sets, from literals and parameters, and one only a single blocking
  assignment of such a constant sets, in an `initial` block with no delay,
  event control, `wait` or task call. Any other write in the design keeps a
  variable, even one that never runs: a procedural or continuous assignment,
  a port connection, `force`, a clocking block drive, `++`, or an argument a
  task, function or system task can write. A write xezim can't trace to its
  variable, such as one through a virtual interface, keeps every variable of
  that name. Writes through VPI don't count, so a variable a cocotb test
  drives stays only if it has no initializer. Ports,
  nets, structs, enums, variables of a named type and an initialized variable
  declared in a generate block are always tracked.
- With `--no-toggle-integers` (or `XEZIM_TOGGLE_INTEGERS=0`), the variables
  of an integer atom type are not tracked either: `int`, `integer`,
  `shortint`, `longint` and `byte` (§6.11), signed or unsigned, declared
  directly or through a typedef, ports included. Vectors such as
  `logic [31:0]`, enums whatever their base type, packed structs and the
  members of unpacked structs are still tracked. Each toggle entry's `type`
  says which entries the option leaves out (see
  [the results](#the-code-coverage-results)).
- Counts are exact: they do not stop at 1.

#### Conditions and expressions

Condition and expression coverage are *focused expression coverage*: a logic
expression is split into its input terms, and each term has two rows, the term
at 0 and the term at 1. A row counts when, in an evaluation, the term has that
value and decides the result: no other term masks it. A term is covered once
both of its rows are; coverage is the covered terms over all terms.

- **Items.** A *condition* is the condition of an `if` (each one of an
  `else if` chain), a `while` or `do ... while`, or a `?:` (wherever `?:`
  is a [branch](#branches), and in the arguments of `$display`, `$write`,
  `$error` and the other message tasks). An *expression* is the value of a
  procedural assignment (blocking or nonblocking), of a continuous
  assignment, or of an input port connected to an expression (`.en(a & b)`,
  an expression of the parent instance), when that value is 1 bit wide.
- **Operators and terms.** The logic operators are `&&`, `||`, `!`, and `~`,
  `&`, `|`, `^`, `~^` on 1-bit operands; in an expression also `?:` with
  1-bit arms, whose condition is one of its terms. Every other operand is a
  term: a signal, a select, a function call, a relation (`a == b`, `x < 3`), a
  reduction (`^v`, and `~^v` its own term), a vector operand of `&&`, a
  bitwise operator on vectors. A 1-bit signal compared with 0 or 1
  (`s == 1'b0`) is the term `s`. The same text twice is one term.
- **Which expressions count.** One with a logic operator on two operands that
  are not constant, or one that is a relation (`if (state == IDLE)`: one
  term). Not: a lone signal or select, possibly negated (`if (!rst_n)`), a
  1-bit signal compared with a constant, a negated relation
  (`!(a == b)`), a `for` loop's condition, a 1-bit `?:` whose arms take more
  bits where assigned, and an expression with a system function as a term
  (`!$isunknown(x) && y`). Constants fold first: `x & 1'b0` is 0, so in
  `(a & b) | (b & 1'b0)` only `a` and `b` are terms. An expression with more
  than 32 terms is not covered. A `?:` that is part of an expression is a
  multiplexer of it, not a condition of its own.
- **Masking.** `&&` and `||` are short-circuit (§11.3.5): the left operand of
  `&&` decides alone when it is 0, as the right operand is not evaluated, and
  decides at 1 when the right is 1; the right operand decides when the left
  is 1. `||` is the same with 0 and 1 swapped. The bitwise operators evaluate
  both operands: one decides when the other is 1 for `&`, 0 for `|`, and
  known for `^` and `~^`. A multiplexer's condition decides unless its other
  arm is a constant equal to the taken arm, and each arm decides when the
  condition selects it. A decision passes up through the operators above it.
  An x or z term gives no row. A negated `&&` or `||` (`!(a && b)`, `~(a ||
  b)`) is the reference simulator's `~&` or `~|`: both operands decide as
  for `&` and `|` (the right one is read even when the left decides, if
  that has no side effects).
- **Bimodal items.** An item with `^`, `~^` or (in an expression) `?:`
  counts each row by the item's value too, as the reference simulator does:
  there a term's two rows can both decide the same value (`a ^ b` at (0, 0)
  and (1, 1) is always 0; `s ? x : y` with `x` and `y` equal), so a term is
  covered when its rows were hit at different values of the item. A design
  unit adds up only the copies of an item that count alike.
- **Transient states.** The reference simulator evaluates an item each time
  one of its inputs changes, so it also sees the delta cycle in which one
  input has its new value and another not yet: in `assign y = d | ~v`, where
  `d` follows `v` through another block, `v` at 1 with `d` still 0. xezim
  settles combinational logic in dependency order and evaluates the item once
  all of its inputs have changed, so for an item of a continuous assignment,
  or at the top level of a combinational `always` block (not under an `if`
  or `case`, nor in a loop), it replays the inputs' changes in the order the
  reference simulator makes them, and counts the rows of each state between.
  A block's output changes one delta cycle after the block's inputs. A net
  driven by continuous assignments (a port connection, a gate) changes half
  a cycle after the variables it reads, but together with the nets it reads;
  an inverter of an `always` block's output (`assign q_n = ~q`) with that
  output. A combinational block runs after the continuous assignments of its
  inputs' cycle, and sees the outputs of the blocks above it in its module
  that ran in that cycle too. Inputs one process writes change together.
  A variable an `assign` drives from the top's inputs only changes with
  them or after them (the testbench writes those through the simulator's
  interface; cocotb's writes change it with them).
  How late a signal changes depends on which signals upstream changed, so
  each input has an earliest and a latest step (the shortest and the longest
  path to it), and a state between two inputs' changes counts only when one
  input's latest step is before the other's earliest: otherwise they count
  as changing together.
- **Counts** are the evaluations in which the row was hit, one per
  evaluation (or replayed state) however many times the term appears.
- **Evaluation.** The interpreter evaluates each item, operator by operator
  as the design would (the right of `&&` is evaluated only when needed, so a
  function call's side effects happen as often as without coverage), so the
  simulation's results do not change. xezim runs combinational blocks after
  each blocking write of a process; the reference simulator once the
  process waits. An item they evaluate meanwhile counts its last such
  evaluation when the process waits (a `#0` too), so the process's writes
  one at a time are not seen. Reading what the design skipped (the other
  operand of a negated `&&`), coverage counts nothing inside it, so the
  rows don't depend on which kinds are collected.

For example, `if (en && q != 3'd7)` evaluated with `en` at 0 hits `en`'s row 0
(the right side is not evaluated); with `en` at 1 it hits `en`'s row 1 when
`q != 3'd7` is 1, and the row of `q != 3'd7` for its value.

#### FSMs

xezim finds the design's finite state machines the way the reference simulator
does, and counts their states and transitions. For example:

```systemverilog
module door (input logic clk, rst, open_req, close_req, output logic busy);
  typedef enum logic [1:0] {CLOSED, OPENING, OPEN, CLOSING} state_t;
  state_t state, next;
  always_ff @(posedge clk)
    if (rst) state <= CLOSED;
    else     state <= next;
  always_comb begin
    next = state;
    case (state)
      CLOSED:  if (open_req)  next = OPENING;
      OPENING: next = OPEN;
      OPEN:    if (close_req) next = CLOSING;
      CLOSING: if (open_req)  next = OPENING;
               else           next = CLOSED;
    endcase
  end
  assign busy = state == OPENING || state == CLOSING;
endmodule

module tb;
  logic clk = 0, rst = 1, open_req = 0, close_req = 0, busy;
  door u_door (.*);
  always #5 clk = ~clk;
  initial begin
    @(negedge clk) rst = 0;
    open_req = 1;  @(negedge clk) open_req = 0;
    repeat (3) @(negedge clk);
    close_req = 1; @(negedge clk) close_req = 0;
    repeat (2) @(negedge clk);
    $finish;
  end
endmodule
```

```text
$ xezim --code-coverage=fsm --verbose door_fsm.sv 2>&1 | grep '^\[COV\] code\|^\[COV\]  '
[COV] code coverage: fsm 8/11 (72.73%)
[COV]   tb.u_door (door): fsm 8/11 (72.73%)
```

The door's entry in `xezim_cov.json` (spread over several lines here):

```json
        "fsm": {"covered": 8, "total": 11, "percent": 72.73, "states": {"covered": 4, "total": 4, "percent": 100.00}, "transitions": {"covered": 4, "total": 7, "percent": 57.14}},
        "fsms": [{"variable": "state", "next": "next", "file": "door_fsm.sv", "line": 4, "reset": ["CLOSED"],
          "states": [{"name": "CLOSED", "value": 0, "line": 10, "count": 3}, {"name": "OPENING", "value": 1, "line": 11, "count": 1},
                     {"name": "OPEN", "value": 2, "line": 12, "count": 2}, {"name": "CLOSING", "value": 3, "line": 13, "count": 1}],
          "transitions": [{"from": "CLOSED", "to": "OPENING", "line": 10, "count": 1}, {"from": "OPENING", "to": "CLOSED", "line": 5, "count": 0},
                          {"from": "OPENING", "to": "OPEN", "line": 11, "count": 1}, {"from": "OPEN", "to": "CLOSED", "line": 5, "count": 0},
                          {"from": "OPEN", "to": "CLOSING", "line": 12, "count": 1}, {"from": "CLOSING", "to": "CLOSED", "line": 14, "count": 1},
                          {"from": "CLOSING", "to": "OPENING", "line": 13, "count": 0}],
          "unreachable": [], "illegal": 0}]
```

The reference simulator reports the same states, transitions, lines and counts
for this design.

**What is an FSM.** A variable is a state machine's state when all of these hold:

- It is a variable of 2 to 64 bits (a vector, an `int` or `integer`, an enum)
  that one edge-triggered always block, the *state process*
  (`always_ff @(posedge clk)`, `always @(posedge clk or negedge rst_n)`),
  assigns as a whole, and no other always block assigns. An initial block or a
  declaration may give it a first value.
- The state process, or the block that computes its next value, tests it: a
  `case (state)`, or an `if` or `?:` that compares it with constants
  (`state == IDLE`, `state != IDLE`, `&&`, `||` and `!` of those,
  `state inside {...}`). At least one assignment of the state (or of its next
  value) must be made inside such a test; a hold (`next = state`) counts.
- Every assignment of it yields constants: a parameter or `localparam`, an enum
  member (a package's too), a literal or a `` `define `` of one, a cast of one
  (`state_t'(IDLE)`), the variable itself, a `?:` of those, or an expression
  of the variable in a branch that fixes its value (`2'd1: cnt <= cnt + 1` under
  `case (cnt)`; such a counter is an FSM, as in the reference simulator).
- Or the state process loads a next-state variable (`state <= next`), which one
  combinational block (`always_comb`, `always @*`, `always @(a or b)`), the
  state process itself, or one continuous assignment computes, under the same
  rule. That is the two-process style of the example.

A register that nothing tests (`mode <= 2'd1;` under unrelated conditions), a
value register that is compared with a constant only to drive something else
(`price <= next_price;` with `next_price` set from a `case` on another signal,
and `if (price == 0) ...` elsewhere), a state loaded from an input or any other
variable, and a cast of a `?:` (`state_t'(sel ? A : B)`) do not make an FSM;
neither does a 1-bit variable.

**States.** The states are the values the assignments yield and the constants
the tests compare with. An enum member that no assignment or test names is not a
state. Each state is named after the parameter or enum member that gives it
when every state has such a name, and the states are `st0`, `st1`, ...
otherwise (the constants the variable is compared with first, in source
order, then the other values assigned). The reference simulator numbers values
assigned under one branch of another test differently (an `else` before its
`if`, a `case`'s items last to first), so such numbered names can differ from
its report; the values and counts do not. A constant's value is its own bits,
zero-extended to the state variable's width even when the constant is signed
(a 2-bit signed `K` of `2'b11` is state 3, also in `state <= K`), and an
unbased unsized literal (`'1`) fills that width, as in the reference simulator.
Each state's `line` is where the first transition out of it starts (the case
item or `if` testing the variable that holds the assignment, preferring a
transition inside such a test), as in the reference simulator's report.

**Transitions.** A transition from S to T is an assignment that yields T in a
branch where the variable can be S. An assignment outside every test of the
variable, such as the reset `state <= CLOSED` or a `next = IDLE` default, is a
transition from every state but its own: the example's reset adds
OPENING -> CLOSED and OPEN -> CLOSED. A `default` case item applies to the
states the other items do not list. An `if`, `?:` or `case` that only
parameters decide (`if (PARITY != 0)`, `case (MODE)`) contributes only the
branch they select, as in the reference simulator: the other branches' states
and transitions are not bins. Such a `case` selects the item simulation would
(the widest of its selector and labels sets the width, `casez`/`casex`
wildcards apply); one with a label that is not a constant contributes all its
items, as in the reference simulator. Assignments that keep the state
(`next = state`, `IDLE: next = IDLE`) are not transitions. A transition's `line`
is the assignment that makes it: one inside a test of the variable before a
reset or default, then one in the state process before one in the next-state
logic, then the last in the source.

**Counting.** A state counts when the state process assigns the variable and
leaves it in that state at the end of the time slot, but only on a run in which
something the process reads (or the state variable itself) changed since its
previous clock edge. That is the reference simulator's count: it skips a
clocked process whose inputs have not changed. So a state held by
`state <= next` counts on the clock that enters it and on the next one (when
`state` has just changed), not on every clock: the example's OPEN, held three
clocks, counts 2, and CLOSED counts the reset clock, the clock after it and two
after CLOSING. A one-process FSM that assigns the state only to change it counts
each visit. A run that an asynchronous reset starts (`posedge rst` in the event
control) counts by the same rule, comparing with the last clock edge's run, and
the next clock edge's run compares with that clock run too. Several assignments
in one time slot count once, as the final value.

A transition counts when a sample finds a state other than that of the previous
sample. The previous sample is the state process's last assignment, or the
variable's value at the end of time 0 (from a declaration's initializer or an
initial block): a value written elsewhere, such as by the testbench or by an
assignment to part of the variable, is not seen.

**Unreachable and illegal states.** A state that no transition reaches from a
reset state is still a state: it counts in the totals, stays uncovered and is
listed in `unreachable`, as in the reference simulator. A value outside the
state set that the state process leaves in the variable (one the testbench
forced in, held by `next = state`) counts in `illegal`, not in the coverage, and
no transition is counted into or out of it. A value with x or z bits (before
reset) is neither.

FSM coverage is the states and transitions together: `fsm` in the results has
the covered and total counts of both, and `states` and `transitions` apart, the
reference simulator's "FSM States" and "FSM Transitions".

#### Scopes

Code in a module body (generate blocks included) belongs to its instance. The
methods of a class and the tasks and functions of a package belong to a scope
named after the package (kind `package`); a class declared outside any package
or module belongs to `$unit` (kind `unit`), and one declared inside a module to a
scope named after the module (kind `module`), shared by all of its instances.

The signals declared in a generate block are listed in its instance under their
names below the block (`u_r[0].r`, `u_if.s`). The names of generate blocks and
instances, instance arrays included, are scopes, not signals, and have no
toggle bins.

#### Compared with other simulators

The rules above follow what other simulators report for the same constructs. The
differences:

- Counts of combinational code (`always_comb`, an `always` whose event
  control lists only level changes such as `@*` or `@(a or b)`, continuous
  assignments and the `?:` in them) are the number of times xezim evaluated
  it, which can be more than another simulator's count. Whether a statement
  or an arm was hit agrees.
- A loop counts once per start. Other simulators may count its iterations or
  its condition tests, and a `for` loop's step as a statement of its own.
- A `case` item listing several expressions is one arm. Other simulators can
  give each expression its own arm.
- An `always` that starts with a delay counts when the delay has passed, not
  when it starts, which can make its count one lower.
- A continuous assignment that another simulator optimizes into a gate
  (`assign y = r;`, `wire w = a ? b : c;`, or `assign y = P ? a : b;` once
  its constant `?:` is gone) is still a statement here, and a `?:` in it that
  is not decided by a constant still a branch.
- An input port tied to a parameter or to an expression (`.en(P == 1)`) is
  not a [constant](#constant-conditions) here, as one tied to a literal is.
  Other simulators can propagate those constants into the instance and leave
  out the code they rule out, as well as the code of conditions on real
  parameters.
- A design unit counts each arm of an `if` chain that any of its instances
  counts. Where constant conditions leave instances different arms of one
  chain, another simulator's design-unit view can line those arms up by
  position instead, and count fewer.
- Enum variables toggle as bit vectors, not by value, and unused variables stay
  in the toggle list.
- A variable stays tracked, where the reference simulator leaves it out as
  fixed, when its only other write is under a constant condition that never
  holds (`if (0)`), from another module through a hierarchical name, or to a
  part of it (`v[3:0] = 0`), or when it is a 2-state variable initialized to
  0 or one initialized to all x, which xezim can't tell from one nothing
  writes. So does one that only a class writes through a virtual interface.
  A net whose only driver is a constant stays tracked too.
- Variables of an integer type (`int`, `integer`, `shortint`, `longint`,
  `byte`) are tracked, as some simulators do by default; others leave them
  out. `--no-toggle-integers` leaves them out. It matters most for a loop
  counter declared in a module (`integer i;` for a `for` loop): its 64 bins
  rarely toggle, since its high bits stay 0 and it ends each time slot at the
  same value.
- An input port is always its own entry in its instance. A simulator that
  optimizes for speed can merge a port connected straight to a parent's net
  into that net, and then lists it under the parent only; with full signal
  visibility it lists the port in the instance, as xezim does.
- Toggle counts are not capped at 1.
- Condition and expression items match the reference simulator's focused
  expression coverage report item for item, term for term and row for row on
  most designs. The differences:
  - Another simulator folds what it can prove constant: a variable assigned
    only once, an input port tied to a constant (here only a constant written
    directly in the port connection folds), or a `for` loop's index. Here the
    items of such code still count. The items of code that
    [constant conditions](#constant-conditions) rule out are excluded here
    too.
  - Transient states (see [Conditions and expressions](#conditions-and-expressions))
    follow a model of the reference simulator's delta cycles, measured on
    small designs, not its scheduler. Not replayed: an item under an `if` or
    `case` or in a loop of a combinational block (the reference simulator
    counts those states too when the item runs in them), in a function or
    task, with a function call in a term, or with a term that reads what its
    own block writes; nor a state between two inputs whose order depends on
    which signals upstream changed: with `assign n = p & en`, `p` a block
    output and `en` a register, `n` changes after `p` when `p` changes and
    on its own when `en` does, so the state between `p`'s change and `n`'s,
    which the reference simulator counts, is not counted.
  - Each iteration of a generate loop has its own items, as there. Those of
    procedural code name their generate block (`block`); those of continuous
    assignments share `file`, `line` and `text`.
  - Counts are evaluations, like the other kinds; the other simulator may
    only record whether a row was hit.
- A state process's input check compares values at its runs and at the end of
  each time slot, and follows the next-state variable through each run of its
  block (`next = go ? A : B; if (state == A) next = state;` changes `next` and
  changes it back in one run, which counts as a change). Any other input that
  changes and changes back within one time slot goes unseen, so a held state
  can count once less than in the reference simulator. The clock is the edge in the event control on a signal the body
  does not read; with none or several, every run counts as a clock run. An
  assignment to part of the state variable (`state[1] <= 1'b1`) is not a state
  assignment; the reference simulator counts a visit of the current state for
  it.
- The `line` of a state with no transition out of it made inside a test of the
  variable can differ from the reference simulator's, which takes it from its
  own ordering of the transitions.

### The code coverage results

The results file gains a `code_coverage` member:

| Field | Meaning |
|---|---|
| `code_coverage.kinds` | The kinds collected: `statement`, `branch`, `toggle`, `condition`, `expression`, `fsm` |
| `code_coverage.statement`, `.branch`, `.toggle`, `.condition`, `.expression`, `.fsm` | Totals over the design: `covered`, `total` and `percent` (two decimals; `null` when `total` is 0). Toggle totals count bins, two per bit; condition and expression totals count terms. FSM totals count states and transitions, and `fsm` also has `states` and `transitions`, each with its own `covered`, `total` and `percent`. `excluded`, present when it is not 0, counts the statements or arms left out of `covered` and `total` (see [Constant conditions](#constant-conditions)) |
| `code_coverage.scopes[]` | One entry per scope that has code: the instances by path, then the packages, `$unit` and the modules that declare classes |
| `scopes[].scope` | The instance path from the top (`tb.u_cnt`), or the package, `$unit` or module name |
| `scopes[].design_unit` | The module or interface of the instance; the scope's own name otherwise |
| `scopes[].kind` | `instance`, `package`, `unit` or `module` (see [Scopes](#scopes)) |
| `scopes[].statement`, `.branch`, `.toggle`, `.condition`, `.expression`, `.fsm` | The scope's totals, like the design totals |
| `scopes[].statements[]` | Every counted statement, count 0 included, in source order: `file`, `line`, `kind` and `count`. `kind` is `statement`, `assign`, `always`, `always_comb`, `always_ff`, `always_latch`, `for`, `foreach`, `while`, `do`, `repeat`, `forever`, `delay`, `event` or `wait`. An excluded statement has `"excluded": "constant"` and is not in the totals |
| `scopes[].branches[]` | Every branch: `file`, `line`, `kind` (`if`, `case`, `casez`, `casex`, `case inside`, `randcase` or `ternary`) and `arms[]`, each with `line`, `arm` (`if`, `else if`, `else`, `else (implicit)`, `item`, `default`, `default (implicit)`, `true` or `false`) and `count`. `"excluded": "constant"` after `kind` marks a branch whose decision is a constant or that is in an excluded arm: none of its arms is in the totals. On an arm, it marks one that can never run |
| `scopes[].toggles[]` | Every tracked signal: `signal` (its name within the scope), `width`, `type`, and `rise[]` and `fall[]` with one count per bit, bit 0 (the rightmost) first. `type` is the data type the signal was declared with, after any typedef: `logic` (also a net declared without a data type), `bit`, `reg`, `byte`, `shortint`, `int`, `longint`, `integer`, `enum`, `struct` or `union`. A member of an unpacked struct or union has the type of the variable it belongs to, `struct` or `union`. `--no-toggle-integers` leaves out the entries of type `byte`, `shortint`, `int`, `longint` and `integer` |
| `scopes[].conditions[]`, `.expressions[]` | Every condition and expression item, never-evaluated ones included: `file`, `line`, `kind` (a condition's `if`, `while`, `do` or `ternary`; an expression's `statement`, `assign` or `port`), `block` (the generate block within the scope, when there is one), `text` (the source text) and `terms[]`, each with `term` (its source text), `hits` (the counts of its rows 0 and 1) and, in a bimodal item (one with `^`, `~^` or `?:`), `outputs` (`[[row 0 at 0, row 0 at 1], [row 1 at 0, row 1 at 1]]`, by the item's value). An item in code that [constant conditions](#constant-conditions) rule out is neither listed nor in the totals |
| `scopes[].fsms[]` | Every FSM (see [FSMs](#fsms)): `variable` (the state variable's name within the scope, `gen[0].state` in a generate block), `next` (the next-state variable, or `null` for a one-process FSM), `file` and `line` of the state process, `reset` (the states it is reset to), `states[]` (`name`, `value`, `line`, `count`) in value order, `transitions[]` (`from`, `to`, `line`, `count`) by source state and then target, `unreachable` (state names) and `illegal` |
| `code_coverage.design_units[]` | One entry per module, interface or package: `design_unit`, `instances`, the totals, and `statements`, `branches`, `toggles`, `conditions`, `expressions` and `fsms` with the counts of all its instances added up (FSMs of different parameterizations stay apart). A statement, arm, bin, row, state or transition is covered when any instance covered it. A statement or arm is excluded only when every instance excludes it; otherwise its count adds up only the instances that count it (the arm a constant decision takes runs, but it is no arm in an instance where the decision is made at run time) |

Only the kinds collected appear. Several statements on one line are separate
entries with the same `line`.

With `--verbose`, the end of the run prints the `[COV]` lines of the
[example](#an-example) as well.

### Adding code coverage runs together

Each run's file lists every statement, arm, bit, state and transition, hit or
not, in the same order for the same design, so the files of several runs add up. For example, one
random `case` selector per run:

```systemverilog
module tb;
  bit [1:0] op;
  int acc;
  initial begin
    op = $urandom_range(0, 3);
    case (op)
      0: acc = 1;
      1: acc = 2;
      2: acc = 3;
      default: acc = 4;
    endcase
    $display("op = %0d", op);
  end
endmodule
```

```text
$ mkdir -p cov
$ for s in 5 6 7; do XEZIM_COV_DB=cov/seed$s.json xezim --code-coverage=stmt,branch +seed=$s op_cov.sv; done
op = 2
Simulation finished at time 0
op = 3
Simulation finished at time 0
op = 0
Simulation finished at time 0
```

Each run covers 3 of the 6 statements and 1 of the 4 arms. Keyed by scope and
position, the statements of all three runs add up with `jq`:

```text
$ jq -s '[.[] | .code_coverage.scopes[] | .scope as $s | .statements | to_entries[]
          | select(.value.excluded | not)
          | {key: "\($s)#\(.key)", count: .value.count}]
         | group_by(.key) | map(map(.count) | add)
         | {covered: map(select(. > 0)) | length, total: length}' cov/seed*.json
{
  "covered": 5,
  "total": 6
}
```

and the branch arms the same way:

```text
$ jq -s '[.[] | .code_coverage.scopes[] | .scope as $s | .branches | to_entries[]
          | .key as $b | select(.value.excluded | not) | .value.arms | to_entries[]
          | select(.value.excluded | not)
          | {key: "\($s)#\($b)#\(.key)", count: .value.count}]
         | group_by(.key) | map(map(.count) | add)
         | {covered: map(select(. > 0)) | length, total: length}' cov/seed*.json
{
  "covered": 3,
  "total": 4
}
```

Only `1: acc = 2;` never ran. The `select`s leave out the
[excluded](#constant-conditions) statements and arms, which are the same in
every run of a design. Toggles add up per bit in the same way, from
`.toggles[] | .rise` and `.fall`, and FSMs per state and transition, from
`.fsms[] | .states` and `.transitions`.

### Code coverage limits

- No exclusions besides code that [constant conditions](#constant-conditions)
  rule out: every other statement, arm, term, tracked bit, state and
  transition counts.
- FSMs that are not recognized: one-hot machines written as `case (1'b1)` over
  the state bits, `casez`/`casex` items with wildcards, a state variable wider
  than 64 bits or of one bit, an element of an array or a member of a struct, a
  state computed by a function call, next-state logic spread over several
  blocks, and assignments made in a task or function the state process calls.
  The reference simulator also takes for a state machine a register loaded
  from a counter under a test of it inside another state machine's branches
  (`root <= next_free;` under `if (root == NULL)` in a `case (build_state)`
  item); xezim does not.
  The FSM extraction of the reference simulator recognizes some of these.
- Instrumented code leaves some of xezim's fast paths: the optional native code
  generators, gate fusion for instrumented continuous assignments, table
  lookups for `case` statements, skipping a flop's clock edge when its inputs
  are unchanged, clock generators such as `always #5 clk = ~clk;`, and parallel
  evaluation. The interpreter evaluates condition and expression items. The
  results do not change; the run is slower.
- Condition and expression coverage leave out: vector operands of bitwise
  operators (`a[3:0] & b[3:0]` is one term, and as a value no item),
  operands of relations (`a == b` is one term, not compared bit by bit),
  `case` items, `for` loop conditions, `wait` conditions, expressions wider
  than 1 bit, `return` values and the arguments of calls (a `?:` condition in
  them is a condition, though). An operand whose width xezim cannot tell
  without evaluating it (a class property, a method's result) is a term.
- A function evaluated during elaboration (for a parameter value) counts only its
  calls during the run.
- Assertion action blocks, `randsequence` productions and covergroups are not
  instrumented, and a `?:` elsewhere than listed under [Branches](#branches)
  (in a system task's arguments, a loop condition or a `case` selector) is not
  a branch.
- A run from a precompiled artifact (`-o`) has no source text, so its entries
  have an empty `file` and line 0. The ANSI ports of the instances below the
  top then have type `logic` unless declared through a typedef, and
  `--no-toggle-integers` keeps them.
- The file lists every statement and tracked bit of the covered scopes. For a
  large design, cover the design under test only (`--code-coverage-scope`), or
  leave toggles out (`--code-coverage=stmt,branch`).

---

## Limits and unsupported features

**Not supported at all:**

- Code coverage is statement, branch, condition, expression, toggle and FSM;
  see [Code coverage limits](#code-coverage-limits).
- Coverage databases in other formats (such as UCIS), text or HTML coverage
  reports, and merging results across runs (the files can be added up with
  `jq`: see [Several runs](#several-runs) and
  [Adding runs together](#adding-code-coverage-runs-together)).
- The coverage system functions `$coverage_control`, `$coverage_get`,
  `$coverage_get_max`, `$coverage_merge`, `$coverage_save`, `$set_coverage_db_name`
  and `$load_coverage_db`. They print `Warning: unknown system task '<name>' ignored`
  once, and the functions return 0.
- `coverage save` and `coverage report` in a `-do` script: ignored with a warning;
  the results are in `xezim_cov.json`.

#### Where a covergroup can be declared

A covergroup can be declared in any module (the top or one instantiated below it,
and every top of a design with several), an interface, a package, at file scope,
and in a class wherever the class is declared.

A covergroup declared in a module or interface is a separate type in each instance
of that module: `get_coverage()` averages the covergroup's instances within that
module instance only, and `$get_coverage()` counts each module instance's type
once. A covergroup declared in a package is one type however many modules create
it.

**Covergroup features that are accepted but have no effect, or only a partial one:**

| Feature | What happens |
|---|---|
| `bins name[] = {...} with (expr)` on a coverpoint | The `with` filter is ignored: the bin holds every listed value |
| `bins name = default sequence` | Counted like `default`: every sampled value no other value bin holds |
| An `ignore_bins` or `illegal_bins` transition | An `illegal_bins` transition is reported; neither removes the transition from other bins |
| Automatic bins of a signed coverpoint | They cover 0 to 2^width-1: a negative value falls in no automatic bin |
| `matches` in a cross bin select | Ignored |
| A file-scope covergroup created in a module instance | A separate type per module instance, like a covergroup declared in the module |
| `option.per_instance`, `option.goal`, `option.name`, `option.comment` | Readable as `cg.option.goal` (the covergroup's setting, a value written at run time, or the default), but they change nothing else. An option written at run time (`cg.option.at_least = 2`) does not change the coverage either, as in the reference simulator. The results file always has one entry per instance, named after the type |

---

## Quick reference

| Flag / variable | Effect |
|---|---|
| (none) | Covergroups and assertions are always collected. A run with any of them writes `./xezim_cov.json` |
| `XEZIM_COV_DB=<path>` | Write the results file to `<path>`; `/dev/null` to skip it |
| `--verbose`, `XEZIM_VERBOSE=1` | Print the `[COV]` summary lines on stderr, along with the other engine lines |
| `+seed=<n>` | Seed the random generator, for reproducible random stimulus and coverage |
| `--error-exit` | Exit nonzero after any `$error`, including one from an assertion's action block, and after an `illegal_bins` hit |
| `--code-coverage[=<kinds>]`, `XEZIM_CODE_COVERAGE=<kinds>` | Collect code coverage: `stmt`, `branch`, `cond`, `expr`, `toggle`, `fsm` or `all` (the default). See [Code coverage](#code-coverage) |
| `--code-coverage-scope=<path>[,<path>...]` | Only cover these instance subtrees and packages |
| `--no-toggle-input-ports`, `XEZIM_TOGGLE_INPUT_PORTS=0` | Leave input and inout ports out of toggle coverage; by default each instance counts its own (`--toggle-input-ports`). See [Toggles](#toggles) |
| `--code-coverage-constant=<exclude\|keep>`, `XEZIM_CODE_COVERAGE_CONSTANT` | Leave out (`exclude`, the default) or count (`keep`) the code that [constant conditions](#constant-conditions) rule out |
| `+cover`, `+cover=<letters>`, `-coverage [<types>]` | Other simulators' spellings of `--code-coverage`: `s`, `b`, `c`, `e`, `t` and `f` (or `B`, `E`, `T` and `F`) select the kinds; other letters are ignored with a warning |
| `-covworkdir <dir>`, `-covscope <name>`, `-covtest <name>` | Write the results file to `<dir>/<scope>/<test>/xezim_cov.json` (defaults `cov_work`, `scope`, `test`) |
| `-covoverwrite` | Accepted for compatibility with other simulators; no effect (the file is always replaced) |
| `--no-toggle-integers`, `XEZIM_TOGGLE_INTEGERS=0` | Leave variables of an integer type (`int`, `integer`, `shortint`, `longint`, `byte`) out of toggle coverage. `--toggle-integers` keeps them (the default) |
| `+fcover` | Accepted for compatibility with other simulators; no effect |
| `-do "coverage save ..."`, `-do "coverage report ..."` | Ignored with a warning |
