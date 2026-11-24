# Type specification `tb`: testbench

A testbench is one version of a way of measuring an interface: a netlist, the command that runs it, and an analysis script that turns the results into metrics. Anyone may write testbenches for any interface, and an interface may have many. The prefix is `tb`. This specification needs the common rules of TOA `0.1` or later (chapters 2 to 8); the definition of its fields is `types/tb.schema.json`.

## Fields

Besides the common fields (3.2.1):

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `type` | must | `tb` | The type |
| `ifc` | must | an object name with the prefix `ifc` | The interface it measures |
| `name` | must | `tb_[a-z0-9_]{1,36}` | The name of the testbench, such as `tb_static`. Its generations share it, and it marks their series (3.4). It is the first part of the full names of its metrics |
| `socket` | must | `[A-Za-z_][A-Za-z0-9_]{0,63}` | The name of the subcircuit that its netlist uses for the block measured (below) |
| `probes` | may | a list of names of probes of the interface | The probes its netlist names (below) |
| `files` | must | a mapping | Its files by role, as for circuits (`ckt`): `netlist` must list at least one, the first being the netlist that is run; `va`, `rtl`, `packages` and `extras` may be written |
| `external` | may | a list | External files it needs but does not carry (3.5). A testbench names no process |
| `run` | must | a string | The command that runs the simulation, as a reference; the user's own setup may differ |
| `analyze` | must | a mapping | The analysis script: `entry`, `python` (the versions of Python it needs, such as `'>=3.10'`) and `results` (below) must be written, `requirements` may be |
| `metrics` | must | a list of one or more metrics | What it measures (below) |
| `simulators` | may | a list of `{name, version}`, `version` being optional | Simulators the author has run it with. For reference only: a tool must not refuse to run it with another |
| `parameters` | may | a mapping | The parameters that the testbench's own netlist uses, such as the amplitude of a stimulus, a load or the supply; not those of the circuit measured. Each is `{unit, description}` |
| `presets` | may | a mapping from names to a set of conditions or a list of sets | Named conditions to run it with (below) |

Each metric:

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `id` | must | `[a-zA-Z][a-zA-Z0-9_]{0,31}` | Its name. Its full name is `<name>.<id>`, such as `tb_static.inl` |
| `unit` | must | a string | Its unit |
| `better` | must | `lower`, `higher` or `none` | Which way is better |
| `tolerance` | must | `{abs}`, `{rel}` or both, numbers | How far apart two results may be and still agree (below) |
| `description` | may | a string | How it is measured |

- **The block measured.** The netlist instantiates the block measured as a subcircuit named `socket`, such as `socket: L3_comparator_V2`, and does not define it. Its ports are the pins of the interface, in their order, a pin of `width` N giving N ports. When a run is assembled, the subcircuit of the circuit tested takes this name (7.6). The name is not `dut`, which in an evidence record is an object name.
- **Probes.** The netlist names nothing inside the block measured except the probes of the interface (`ifc`), each written as `<instance>.<probe>`, where `<instance>` is an instance of `socket`, such as `I0.latch_p`. Which net of a circuit a probe is, the evidence record says for each run (`evd`); when a run is assembled, the tool writes that net in place of the probe (7.6). So the testbench runs unchanged on every circuit of the interface, whatever the circuit calls its nets.
- **`analyze.results`** is the format of the results the analysis script reads: `psfascii` (PSF ASCII, a directory of files as Spectre writes it) or `nutascii` (the ASCII raw file of SPICE, one file `results.raw` holding every analysis). Later versions may allow others. The tool makes the simulator write its results in this format; `run` is only a reference.
- **Tolerance:** two results `a` and `b` of a metric agree when `|a − b| ≤ max(abs, rel × max(|a|, |b|))`, an `abs` or `rel` not written counting as 0. Neither result is taken as the reference.
- **Presets.** A set of conditions has the three slots `parameters`, `options` and `models`, written as the evidence record writes its `conditions` (`evd`): every value is single, never a list. A preset is one set, or a list of sets; each set is written in full, and takes nothing from another set or another preset. Each set gives a value to every parameter of `parameters`, and to no other. Each set is run once and gives one evidence record, so a sweep is written as a list with one set per point.
- **A full name keeps its meaning.** Within the generations of one testbench, the meaning of a metric's name never changes; a metric that means something else takes another name. The targets of a circuit (`specifications.targets` of `ckt`) and the requirements in the `uses` of circuits are written with full names.
- **`description`** (3.2.1) says what the testbench measures and how.

## Files

| File | Requirement | Content |
|---|---|---|
| `toa.yaml` | must | The manifest |
| the files listed in `files` | must | The netlists and the files they need |
| the `entry` of `analyze` | must | The analysis script |
| the `requirements` of `analyze` | when written | The Python packages it needs, each with an exact version |
| `README.md` | should | What it measures and how, for people |

- **The netlists** are not run alone: they hold no model files and no circuit of the block measured, they do not declare the parameters of `parameters`, and they set none of the options that the presets set: the tool writes these with the conditions of each run (7.6). A netlist in SPICE holds no `.end`.
- **The analysis script** is called as `<entry> <results> <output>`. It reads the results of the simulations of one set of conditions from the directory `<results>`, and writes into `<output>` a file `metrics.json` (one number per metric, by `id`) and a directory `evidence/`.
- **`conditions.json`:** before calling the script, the tool writes into `<results>` a file `conditions.json`, a JSON object holding the `conditions` of the evidence record it is making, and its `probes` when it has any. The script may use them, for example for a threshold of half the supply.
- **It is someone else's code** (chapter 11). A tool must not run it without the user's agreement, should show it, and its difference from the previous version, before running a version for the first time, and should run it in a process and directory of its own. This is no isolation: a testbench that has not been reviewed should run in a container or virtual machine.
- A file may start with a licence header, as for circuits (`ckt`).
- **Source lists** follow testbenches further, through `ifc` (5.2).

## Checks at sealing

1. Every file listed in `files`, the `entry` and, when written, the `requirements` exist in the object, and no path of `files` is listed twice.
2. The `id`s of the metrics are unique.
3. When `ifc` can be found, it is an interface, and every probe of `probes` is one of its probes.
4. The netlists keep the rules for netlists of circuits (`ckt`, checks at sealing, item 3).
5. The netlists instantiate a subcircuit named `socket` and define none of that name. When the tool can read them, every instance of it has as many ports as the interface has pins, counting `width`.
6. Every name of the form `<instance>.<name>` in the netlists, where `<instance>` is an instance of `socket`, has a probe of `probes` as `<name>`.
7. The netlists declare none of the parameters of `parameters`, and set none of the options that the presets set.
8. Every set of every preset keeps the rules above.

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
socket: strongarm_comp
probes: [latch_p, latch_n]
files:
  netlist:
  - tb.scs
simulators:
- {name: spectre, version: '21.1'}
run: spectre input.scs +aps -format psfascii -raw psf
analyze: {entry: analyze.py, python: '>=3.10', requirements: requirements.txt, results: psfascii}
metrics:
- {id: offset, unit: mV, better: lower, tolerance: {abs: 0.2}}
parameters:
  vdd: {unit: V, description: Supply voltage}
presets:
  nom:
    parameters: {vdd: 1.8}
    options: {temp: 27}
    models: [{file: models.scs, section: tt}]
  supply:
  - parameters: {vdd: 1.62}
    options: {temp: 27}
    models: [{file: models.scs, section: tt}]
  - parameters: {vdd: 1.98}
    options: {temp: 27}
    models: [{file: models.scs, section: tt}]
```
