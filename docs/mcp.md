# The MCP server

`xezim --mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io)
on stdin and stdout, so a coding agent can compile, simulate and measure a
design through tool calls instead of parsing command lines and logs. It speaks
protocol revisions 2025-06-18, 2025-03-26 and 2024-11-05.

* [Registering the server](#registering-the-server)
* [How a call runs](#how-a-call-runs)
* [Arguments every tool takes](#arguments-every-tool-takes)
* [compile](#compile) · [simulate](#simulate) · [coverage](#coverage) ·
  [assertions](#assertions)

## Registering the server

An MCP client starts the server as a local command with no other arguments:

```json
{ "mcpServers": { "xezim": { "command": "xezim", "args": ["--mcp"] } } }
```

The tools run in the server's working directory unless a call names another
(`cwd`), so start the server in the project, or pass `cwd` with each call.

## How a call runs

* Each tool call runs this same `xezim` executable as a child process, in
  `cwd`, with its stdout and stderr going to a log file. A crash, a hang or a
  runaway allocation costs that call only.
* Calls run in parallel. A call stops after `timeout` seconds (600 by
  default), and a `notifications/cancelled` for its request stops it at once;
  either kills the child and everything it started. A cancelled call gets no
  reply.
* When stdin closes, the server answers the calls still running, then exits,
  so requests can be piped in. Killing the server stops its calls (on Linux,
  their children die with it).
* The log, and the results file (`xezim_cov.json`) a run writes, go to a
  directory of their own under the system's temporary directory. The result
  gives the log's path and its last lines; the server removes the directory
  when it exits.
* A failing design is a result, not an error: `passed` or `ok` is false and
  the diagnostics say why. A tool error (`isError`) means the call itself was
  wrong: a missing argument, a `cwd` that does not exist, a Python without
  cocotb.
* Each result is a JSON object, in `structuredContent` and, as text, in
  `content`.

## Arguments every tool takes

| Argument | Meaning |
|---|---|
| `cwd` | Directory to run in; relative paths resolve against it. Default: the server's working directory |
| `sources` | Source files, in compile order |
| `top` | The top-level module. Default: found automatically |
| `include_dirs` | Include directories (`-I`) |
| `defines` | Macros, as `NAME` or `NAME=VALUE` (`-D`) |
| `parameters` | Top-level parameter overrides, as `NAME=VALUE` (`-g`) |
| `args` | More xezim arguments, from the [command-line reference](command-line.md): `["-f", "files.f"]`, `["--sv2017"]`, `["--compat"]` |
| `timeout` | Seconds before the run is stopped. Default: 600 |
| `log_lines` | Lines of the log's end to return. Default: 40 |

A call needs `sources` or `args`. The tools that simulate also take:

| Argument | Meaning |
|---|---|
| `plusargs` | Plusargs: `["+VERBOSE", "+N=4"]` |
| `max_time` | Simulated time limit, such as `10us`, or `none` (`--max-time`). Default: 100 ms |
| `seed` | Random seed (`-sv_seed`, and cocotb's seed). Default: 1 |
| `cocotb` | Run a cocotb test module against `top` instead of an SV testbench: `module` (required; comma-separated for several), `filter` (a regular expression; only the tests whose names match run) and `python` (a Python with cocotb 2; default `python3`) |

A cocotb run loads cocotb's VPI library into xezim and sets the variables
cocotb's own runners set. The test modules must be importable from `cwd`, or
from the server's `PYTHONPATH`.

## compile

Parses and elaborates the design without simulating it (`--compile`).

| Result | Meaning |
|---|---|
| `ok` | The design compiled |
| `exit_code` | xezim's exit status |
| `errors`, `warnings` | How many of each the log has |
| `diagnostics` | The errors and warnings: `severity` (`error`, `fatal` or `warning`), `message`, and `file`, `line` and `column` where the message gives them. At most 50 |
| `log`, `log_tail` | The log's path, and its last lines |
| `timed_out`, `signal` | Whether the run hit `timeout`; the signal that ended it, if one did |

## simulate

Simulates the design with `--error-exit`, so `$error` fails the run as `$fatal`
does.

| Result | Meaning |
|---|---|
| `passed` | The run exited 0 in time, no assertion failed, and for a cocotb run at least one test passed and none failed |
| `sim_time`, `finished` | The time the run ended, and whether `$finish` ended it (else the time limit or an empty event queue did) |
| `diagnostics` | As for `compile`, plus each `** Error`, `** Fatal` and `** Warning` message of the run with its `time` and `scope` |
| `assertions` | Counts by status (see [assertions](#assertions)) and the first 20 failing assertions; `null` when the design has none |
| `tests` | cocotb runs only: `total`, `passed`, `failed`, `skipped`, and the `cases` with `name`, `module`, `status` and a failure's `message` |

and `exit_code`, `errors`, `warnings`, `log`, `log_tail`, `timed_out` and
`signal` as for `compile`.

## coverage

Code coverage per instance, with the bins still uncovered. It simulates the
design with `--code-coverage`, or reads the results file of an earlier run
given as `db`: an `xezim_cov.json`, or a directory holding one.

| Argument | Meaning |
|---|---|
| `db` | Read this results file instead of running |
| `counting` | `xezim` (default) counts by xezim's full rules. `reference` counts as the reference simulator does by default: no enum or generate-block toggles, no bits tied to constants, only `&&` and `\|\|` terms in conditions, straight-line blocks instead of statements, and no FSM reset transitions (see [Counting rules](coverage-guide.md#counting-rules)). Runs only: a results file keeps the rules its run used |
| `coverage_scope` | Only instrument these instance subtrees, such as `tb.dut` (`--code-coverage-scope`) |
| `instance` | Only report this instance and its subtree: a dotted path (`tb.dut`) or a leaf name (`dut`) |
| `max_holes` | Uncovered bins to list. Default: 100 |

| Result | Meaning |
|---|---|
| `run` | The run's result, as `simulate` gives it, when the tool ran one |
| `counting` | The counting profile of the run |
| `db` | The results file read |
| `total` | Design-wide totals per metric: `covered`, `total` and `percent` |
| `instances` | Each instance, and each scope above one, depth first: `instance`, `design_unit`, `metrics` (its own counts), `subtree` (its counts and those of every instance below it), `average` and `holes` (its own uncovered bins per metric) |
| `holes` | The uncovered bins, assertions first and toggles last, at most `max_holes` |
| `holes_total` | How many uncovered bins there are |

The metrics are xezim's `statement` (blocks under `counting: reference`),
`branch`, `condition`, `expression`, `fsm` (states and transitions) and
`toggle` (two bins per bit), plus `assertion`: an assert or assume counts as
covered once it has passed nonvacuously and never failed, and a cover once it
has matched. Covergroups are not in the totals. `average` is the reference
simulator's overall average: the mean of the subtree's statement, expression
(conditions and expressions together), toggle, FSM and assertion
percentages, each where the subtree has any.

Each hole has `instance` and `metric`, and:

| Metric | Fields |
|---|---|
| `statement` | `file`, `line`, `kind`: a statement that never ran |
| `branch` | `file`, `line`, `kind` (`if`, `case`, ...), `arm`: an arm never taken |
| `condition`, `expression` | `file`, `line`, `text`, `term`, `missing`: the values (`false`/`true`, or `0`/`1`) the term never decided |
| `fsm` | `variable`, and `state` or `transition` (`A -> B`), `line`: never reached or taken |
| `toggle` | `signal` (with its bit), `missing` (`rise`, `fall`) |
| `assertion` | `name`, `kind`, `file`, `line`, `status` |

Code that a constant condition rules out is in neither the totals nor the
holes (see [Constant conditions](coverage-guide.md#constant-conditions)).

## assertions

Every assert, assume and cover statement per instance, from a run or from
`db`. Takes `db` and `instance` as `coverage` does.

| Result | Meaning |
|---|---|
| `assertions` | One entry per statement and instance: `instance`, `name`, `kind`, `concurrent`, `file`, `line`, `attempts`, `pass` (nonvacuous successes; a cover's matches), `vacuous`, `fail`, `disabled` and `status` |
| `summary` | How many have each status, and the first 20 failing ones |
| `run`, `db` | As for `coverage` |

The counts are those of the results file's
[per-instance assertion entries](coverage-guide.md#per-instance-assertion-entries).
`status` is, for an assert or assume, `failed` (it failed at least once),
`passed` (it passed nonvacuously and never failed), `vacuous` (it only passed
vacuously) or `unattempted` (no attempt ran to an end: none started, all were
disabled, or they were still running when the run ended); and for a cover,
`covered` or `uncovered`.
