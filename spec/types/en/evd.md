# Type specification `evd`: evidence record

An evidence record is what someone got by examining a design: which object was examined, how, under which conditions, and what came out. The prefix is `evd`. Its `kind` says which kind of evidence it is. Version `0.1` defines two kinds: `sim`, a simulation, in which testbenches were run on a circuit; and `mc`, a Monte Carlo simulation, in which they were run on many samples of the statistical variations of the circuit. Later versions may add others, such as sign-off checks, measurements of silicon and characterisation. This specification needs the common rules of TOA `0.1` or later (chapters 2 to 8); the definition of its fields is `types/evd.schema.json`.

## Fields

Besides the common fields (3.2.1), every evidence record has:

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `type` | must | `evd` | The type |
| `kind` | must | `[a-z][a-z0-9_-]{0,31}` | The kind of evidence. `0.1` defines `sim` and `mc` |

- **A kind a tool does not know:** the tool keeps the record and shows it as it is, and applies only the common rules (chapters 2 to 8) and the two fields above. Other kinds are defined by later revisions of this specification; this does not change what `sim` and `mc` are.

A record of the kind `sim` also has:

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `dut` | must | an object name; in `0.1`, with the prefix `ckt` | The object tested. In `0.1` it is a circuit; later versions may allow other types |
| `tbs` | must | a list of one or more object names with the prefix `tb` | The testbenches run |
| `conditions` | must | a mapping | The conditions actually run, in three slots (below) |
| `probes` | when a testbench lists probes | a mapping from probe names to nets | The net of the circuit tested that each probe is, in this run (below) |
| `environment` | must | a mapping | The environment (below) |
| `metrics` | must | a mapping from full metric names (`<name>.<id>`, see `tb`) to numbers | The value of each metric, as the analysis gave it. Metrics that were not obtained are left out |
| `evidence` | may | a list of `{path, kind}`, with an optional `label` | Files under `evidence/` (below) |

- **The tree tested is the expanded tree of `dut` as it is.** A record cannot use another circuit in place of one of the tree; to test another version of a submodule, make newer versions of the levels above it (7.5) and test that.
- **One record, one point of conditions.** A sweep over conditions gives several records.
- **`conditions`** holds only what may change from one environment to another; the expanded tree, the netlists and the analysis are fixed by `lock.json`:
  - `parameters`: values of the testbench's `parameters`, each a single number, string or boolean, never a list;
  - `options`: settings of the simulator and solver, such as `temp`, `reltol` or `errpreset`;
  - `models`: a list of the model files used, each with `file` (a file name without `/` or `\`), and optionally `section` (a name or a list), `version`, `match` (strings that must all occur in the file), `process` and `sha256` (3.5).
- **`probes`** gives, for each probe that the testbenches of `tbs` list, a net of the circuit in `dut`: a net of its subcircuit, such as `LP`, or a net further down, written as the names of the instances down to it and the net, joined by `.`, such as `xlatch.LP`. It holds those probes and no others. Like `dut` and `conditions`, it is an input of the run: another circuit of the same interface may give other nets to the same testbenches.
- **`environment`:** `simulator` must be written, with `name` and `version`, and optionally `command`: the simulator's own command line that ran it, options included. A local launcher that only starts the simulator, such as a sandbox, a job scheduler or a container, is not part of it and is not written: like a local path, it belongs to the machine. `pdk` (`process`, `release`), `analysis` (`python`, `packages`), `system` (`os`, `cpu`, `threads`) and `note` may be written. Nothing in `environment` holds a local path, a host name or a user name.
- **`evidence`:** the `kind` of a file is `spectrum`, `curve`, `waveform`, `table` or `json`. It is not the `kind` of the record.
- **`description`** (3.2.1) holds anything else a reader should know, such as why a metric is missing.

## The kind `mc`

A record of the kind `mc` is a Monte Carlo simulation: its testbenches were run, under one set of conditions, on many samples of the statistical variations that the models describe. It has the fields of `sim` and keeps their rules, except for these two:

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `conditions` | must | a mapping | As for `sim`, with a fourth slot, `statistics`, which must be written (below) |
| `metrics` | must | a mapping from full metric names to `{n, failed, mean, sigma, min, max}` | The statistics of each metric over the samples (below) |

- **`statistics`** says how the samples were drawn and run:

  | Field | Requirement | Value | Meaning |
  |---|---|---|---|
  | `method` | must | `mismatch`, `process` or `both` | The variations drawn: the mismatch between devices, the variation of the process, or both |
  | `points` | must | a whole number, 1 or more | The number of samples |
  | `sampling` | must | `random`, `lds` or `lhs` | How the samples are drawn: plain random, a low-discrepancy sequence, or Latin hypercube sampling |
  | `seed` | must | a whole number from 1 to 2147483647 | The seed of the random numbers, as actually used; never left to a default |
  | `first` | may | a whole number, 1 or more; 1 when not written | The number of the first sample. The samples are those from `first` to `first + points − 1` |
  | `run` | must | `each` or `batch` | How they were run (7.6): `each`, one simulation for each sample; `batch`, all in one simulation |

  - With `lhs`, the samples are spread over `points` bins, and `first` is 1.
  - The statistical models are in the model files of `models`, as for any run.
- **Sample k** is what a simulation of that sample alone gives, as a record with `first` k, `points` 1 and `run` `each` would hold it. A run with `batch` agrees with it within the tolerance of each metric, but need not be equal to it bit for bit.
- **`metrics`** holds every metric of the testbenches, even one that no sample gave. For each, over the samples of the record:
  - `n` is the number of samples that gave the metric, and `failed` the number that did not, so that `n + failed` is `points`;
  - `mean`, `min` and `max` are written when `n` is 1 or more, and `sigma`, the standard deviation with `n − 1` as divisor, when `n` is 2 or more;
  - the tool computes them from `samples.csv`, never the analysis script, so that they are computed alike everywhere. For a metric whose values are 0 or 1, such as a decision, `mean` is the fraction of samples that gave 1.
- **`evidence/samples.csv`** must be in the record, listed in `evidence` with the kind `table`. It is a CSV file in UTF-8 whose lines end in LF:
  - its first line is `sample` followed by the full name of each metric, in the order of `tbs` and of the `metrics` of each testbench;
  - then comes one line for each sample, from `first` up: its number, then the value of each metric, left empty when the sample did not give it. A sample whose simulation failed has a line with every value empty;
  - a value is written as a decimal number that reads back as the same number.
- **No yield is written.** The targets are in the circuit (`specifications.targets` of `ckt`): anyone can hold `samples.csv` against them, and a new target needs no new run.
- **What the analysis script writes into `evidence/`** for each sample is not kept, except that the tool may keep it for some samples, each under `evidence/samples/<k>/`.

### Comparing two Monte Carlo simulations

Two records of the kind `mc` with the same `tbs`, and the same `conditions` except for `first`, `points` and `run` in `statistics`, are compared in one of two ways:

- **Sample by sample,** when they also have the same `dut` and `probes`, simulators of the same name and version, and, with `lhs`, the same `points`. Their netlists are then the same byte for byte, being fixed by `lock.json`, and so are their samples. Each sample that is in both records is compared metric by metric, by the tolerance of the metric (`tb`); a metric that one gives and the other does not is a difference. They reproduce each other when every sample compared agrees.
- **By statistics,** otherwise. Another circuit gives other samples, even another extraction of the same circuit or its schematic against its extracted netlist; so may another version of the simulator. For each metric that both give with `n` of 2 or more, written below with 1 and 2 for the two records and `t` for the tolerance of the metric applied to the two values compared:
  - the means agree when `|mean₁ − mean₂| ≤ max(t, 3 × √(sigma₁²/n₁ + sigma₂²/n₂))`;
  - the standard deviations agree when `|sigma₁ − sigma₂| ≤ max(t, 3 × √(sigma₁²/(2(n₁ − 1)) + sigma₂²/(2(n₂ − 1))))`;
  - for a metric whose every value in both `samples.csv` is 0 or 1, only the means are compared, by the binomial distribution: they agree when `|mean₁ − mean₂| ≤ 3 × √(p(1 − p)(1/n₁ + 1/n₂))`, where `p = (n₁mean₁ + n₂mean₂)/(n₁ + n₂)`.

  When every metric agrees so, the records are statistically consistent. A tool says so, and does not call it a reproduction.

## The file `lock.json`

Every record of the kind `sim` or `mc` has a `lock.json` at its top (3.3). It records the tree that was simulated, by name, with the SHA-256 of the files used, so that the record can be checked without a TOA tool as well. It is a JSON object:

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `toa` | must | `"lock/1"` | The format |
| `dut` | must | an object name | The same as `dut` in the manifest |
| `tbs` | must | a list of `{object, files}` | Each testbench of `tbs`, in the same order |
| `tree` | must | a list of `{path, object, ifc, files}` | Every circuit of the expanded tree of `dut`, sorted by `path` |

- `path` is the names of subcircuits from the circuit tested down to the instance, joined by `.`, such as `core` or `core.latch`; the circuit tested has the path `""`.
- `object` and `ifc` are object names: the circuit and its interface. `files` maps the path of each file in the object to its SHA-256 in 64 lowercase hexadecimal digits: for a circuit, the files listed in its `files`; for a testbench, the files listed in its `files`, the `entry` of `analyze` and its `requirements`.
- Any member may also have a `label` for people, which tools must not interpret.
- The names in `lock.json` are references (3.3), so the name of the record fixes the whole tree.

## Files

| File | Requirement | Content |
|---|---|---|
| `toa.yaml` | must | The manifest |
| `lock.json` | must, for `sim` and `mc` | The tree simulated (above) |
| `evidence/` | may | Key evidence, such as the data of a plot. Not raw simulation data |
| `README.md` | may | Anything a reader should know |

- **Source lists** do not follow evidence records further (5.2). The source list beside a record lists what the record refers to; a record reached from another object brings nothing more.

## Checks at sealing

For a record of the kind `sim` or `mc`:

1. `lock.json` keeps the format above, and its `dut` and testbenches agree with the manifest.
2. Every `path` of `evidence` is a file under `evidence/` in the object.
3. Every key of `metrics` is `<name>.<id>` of a metric of one of the testbenches, when they can be found.
4. When the objects can be found: `dut` is a circuit; every testbench measures the interface of `dut`; `tree` is the expanded tree of `dut`, and every file hash agrees.
5. Within one expanded tree, each interface has one circuit: two circuits of one interface are two definitions of the same subcircuit, which a simulation rejects.
6. When the testbenches can be found, `probes` names exactly the probes they list.

For a record of the kind `mc`, also:

7. `statistics` keeps the rules above.
8. For each metric, `n + failed` is `points`, and `mean`, `sigma`, `min` and `max` are written as `n` requires.
9. `evidence/samples.csv` keeps the format above, with one line for each sample, and its columns are the metrics of `metrics`. The `n` and `failed` of each metric agree with it.

## Readable name

`evd/<interface name>/<record label>/`, the label being `<UTC time>-<short name>`, for example `evd/strongarm-comp/20261103T1200Z-wrap-off/evd_…/` (6.2).

The readable name without the type directory and the slashes at its ends, such as `strongarm-comp/20261103T1200Z-wrap-off`, is the `label` a tool should write for the record in a source list (5.1), and the name it should show for it. `<interface name>` is the `name` of the interface of the object in `dut`; when that cannot be found, the first 12 characters of the object name in `dut` stand for it. The record label is not in the manifest: the tool that creates the record takes the UTC time of its creation, written as `YYYYMMDDTHHMMZ`, and a short name from the user. A tool showing a record it did not create uses the `label` of its line in a source list, or else `<interface name>/` followed by the first 12 characters of the record's name.

## Example

```yaml
type: evd
license: MIT
kind: sim
dut: ckt_1ec6249c0d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a
tbs:
- tb_ad96a1470e2d4c6b8a9f1e3d5c7b9a0e2f4d6c8b
author: Alice Zhang
probes: {latch_p: LP, latch_n: LM}
conditions:
  parameters: {vdd: 1.8}
  options: {temp: 27}
  models:
  - {file: sky130.lib.spice, section: tt, version: '0.20.1'}
environment:
  simulator: {name: spectre, version: '23.1.0', command: spectre input.scs +aps}
  analysis: {python: '3.11.9'}
metrics:
  tb_offset.offset: 1.7
```

A Monte Carlo simulation of the same circuit:

```yaml
type: evd
license: MIT
kind: mc
dut: ckt_1ec6249c0d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a
tbs:
- tb_ad96a1470e2d4c6b8a9f1e3d5c7b9a0e2f4d6c8b
author: Alice Zhang
probes: {latch_p: LP, latch_n: LM}
conditions:
  parameters: {vdd: 1.8}
  options: {temp: 27}
  models:
  - {file: sky130.lib.spice, section: tt, version: '0.20.1'}
  statistics: {method: mismatch, points: 200, sampling: lds, seed: 12345, run: batch}
environment:
  simulator: {name: spectre, version: '23.1.0'}
  analysis: {python: '3.11.9'}
metrics:
  tb_offset.offset: {n: 199, failed: 1, mean: 0.08, sigma: 1.93, min: -5.12, max: 5.47}
evidence:
- {path: evidence/samples.csv, kind: table}
```
