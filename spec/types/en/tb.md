# Type specification `tb`: testbench

A testbench is one version of a way of measuring an interface: a netlist, the command that runs it, and an analysis script that turns the results into metrics. Anyone may write testbenches for any interface, and an interface may have many. The prefix is `tb`. This specification needs the common rules of TOA `0.1` or later (chapters 2 to 8); the definition of its fields is `types/tb.schema.json`.

## Fields

Besides the common fields (3.2.1):

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `type` | must | `tb` | The type |
| `ifc` | must | an object name with the prefix `ifc` | The interface it measures |
| `name` | must | `tb_[a-z0-9_]{1,36}` | The name of the testbench, such as `tb_static`. Its generations share it, and it marks their series (3.4). It is the first part of the full names of its metrics |
| `files` | must | a mapping | Its files by role, as for circuits (`ckt`): `netlist` must list at least one, the first being the netlist that is run; `va`, `rtl`, `packages` and `extras` may be written |
| `external` | may | a list | External files it needs but does not carry (3.5). A testbench names no process |
| `run` | must | a string | The command that runs the simulation, as a reference; the user's own setup may differ |
| `analyze` | must | a mapping | The analysis script: `entry` and `python` (the versions of Python it needs, such as `'>=3.10'`) must be written, `requirements` may be |
| `metrics` | must | a list of one or more metrics | What it measures (below) |
| `simulators` | may | a list of `{name, version}`, `version` being optional | Simulators the author has run it with. For reference only: a tool must not refuse to run it with another |
| `parameters` | may | a mapping | The parameters that the testbench's own netlist declares, such as the amplitude of a stimulus, a load or the supply; not those of the circuit measured. Each is `{unit, description}` |
| `presets` | may | a mapping | Named sets of conditions in the three slots `parameters`, `options` and `models`, as the evidence record writes them, except that a value of `parameters` may be a list: a sweep, run as one record per point |

Each metric:

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `id` | must | `[a-zA-Z][a-zA-Z0-9_]{0,31}` | Its name. Its full name is `<name>.<id>`, such as `tb_static.inl` |
| `unit` | must | a string | Its unit |
| `better` | must | `lower`, `higher` or `none` | Which way is better |
| `tolerance` | must | `{abs}`, `{rel}` or both, numbers | How far apart two results may be and still agree |
| `description` | may | a string | How it is measured |

- **A full name keeps its meaning.** Within the generations of one testbench, the meaning of a metric's name never changes; a metric that means something else takes another name. Targets of a design, and the requirements in the `uses` of circuits, are written with full names.
- **`description`** (3.2.1) says what the testbench measures and how.

## Files

| File | Requirement | Content |
|---|---|---|
| `toa.yaml` | must | The manifest |
| the files listed in `files` | must | The netlists and the files they need |
| the `entry` of `analyze` | must | The analysis script |
| the `requirements` of `analyze` | when written | The Python packages it needs, each with an exact version |
| `README.md` | should | What it measures and how, for people |

- **The analysis script** is called as `<entry> <results> <output>`. It reads the results of the simulations of one set of conditions from the directory `<results>`, and writes into `<output>` a file `metrics.json` (one number per metric, by `id`) and a directory `evidence/`.
- **It is someone else's code** (chapter 11). A tool must not run it without the user's agreement, should show it, and its difference from the previous version, before running a version for the first time, and should run it in a process and directory of its own. This is no isolation: a testbench that has not been reviewed should run in a container or virtual machine.
- A file may start with a licence header, as for circuits (`ckt`).
- **Source lists** follow testbenches further, through `ifc` (5.2).

## Checks at sealing

1. Every file listed in `files`, the `entry` and, when written, the `requirements` exist in the object, and no path of `files` is listed twice.
2. The `id`s of the metrics are unique.
3. When `ifc` can be found, it is an interface.
4. The netlists keep the rules for netlists of circuits (`ckt`, checks at sealing, item 3).

## Readable name

`tb/<interface name>/<name>.g<generation>/`, for example `tb/strongarm-comp/tb_offset.g1/tb_…/` (6.2). The generation is the one in its `history.json` (3.4).

The readable name without the type directory and the slashes at its ends, `<interface name>/<name>.g<generation>` such as `strongarm-comp/tb_offset.g1`, is the `label` a tool should write for the testbench in a source list (5.1), and the name it should show for it. `<interface name>` is the `name` of the interface in `ifc`; when that cannot be found, the first 12 characters of the object name in `ifc` stand for it.

## Example

```yaml
type: tb
license: CERN-OHL-P-2.0
ifc: ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5
name: tb_offset
title: Input-referred offset by a slow ramp
files:
  netlist:
  - tb.scs
simulators:
- {name: spectre, version: '21.1'}
run: spectre tb.scs +aps -format psfxl -raw psf
analyze: {entry: analyze.py, python: '>=3.10', requirements: requirements.txt}
metrics:
- {id: offset, unit: mV, better: lower, tolerance: {abs: 0.2}}
parameters:
  vdd: {unit: V, description: Supply voltage}
presets:
  nom:
    parameters: {vdd: 1.8}
    options: {temp: 27}
    models: [{file: models.scs, section: tt}]
```
