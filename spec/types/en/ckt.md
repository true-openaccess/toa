# Type specification `ckt`: circuit

A circuit is one version of a design of an interface: a netlist with the files it needs. It refers to the interface it implements, and, through `uses`, to the circuits of other interfaces it is built from. The prefix is `ckt`. This specification needs the common rules of TOA `0.1` or later (chapters 2 to 8); the definition of its fields is `types/ckt.schema.json`.

## Fields

Besides the common fields (3.2.1):

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `type` | must | `ckt` | The type |
| `ifc` | must | an object name with the prefix `ifc` | The interface it implements |
| `name` | must | `[a-z][a-z0-9_-]{1,39}` | The name of the design, such as `sky130-razavi`. Its generations share it, and it marks their series (3.4) |
| `form` | must | `[a-z][a-z0-9_-]{1,31}` | How this circuit itself is written (below) |
| `files` | must | a mapping | Its files by role (below) |
| `external` | may | a list | Other external files it needs but does not carry (3.5) |
| `uses` | may | a mapping | The circuits of other interfaces it is built from (below) |
| `parameters` | may | a mapping from names to numbers, strings or booleans | Parameters of its subcircuit and their defaults |
| `specifications` | may | a mapping | What the author declares of it before any verification: supply, temperature, ranges of pins, topology |
| `origin` | may | a mapping | Where it was taken from, for a design taken from a repository outside TOA (below) |

- **`description`** (3.2.1) says what this design is.
- **`form`** is judged by the content of this circuit's own files, not by what it uses:
  - `ideal`: a model built from ideal elements;
  - `va`: a behavioural model in Verilog-A;
  - `rtl`: Verilog or other RTL;
  - `transistor`: a circuit of transistors, even when it uses other interfaces as well;
  - `passive`: only passive devices of a process, such as a resistor string or a capacitor array;
  - `gates`: a gate-level netlist, only instances of the standard cells of a process;
  - `composition`: only instances of other interfaces and the wires between them, no devices and no model code;
  - `mixed`: several of the above in this level itself.

  Other values may be used; tools that do not know a value show it as it is.
- **`specifications.process`** (3.5) must be written when `form` is `transistor`, `passive` or `gates`, or `mixed` with devices or cells of a process. Circuits of other forms may write it. The circuits of one expanded tree may name different processes.
- **`files`:** each role is a list of paths of files in the object, relative to its top and separated by `/`:

  | Role | Requirement | Files |
  |---|---|---|
  | `netlist` | must, at least one | Netlists; the first defines the subcircuit of the interface |
  | `va` | may | Verilog-A |
  | `rtl` | may | Verilog and other RTL |
  | `packages` | may | RNL packages |
  | `extras` | may | Other files the netlists need |

- **`uses`** maps the name of a subcircuit that the netlist instantiates to the circuit used for it:

  | Field | Requirement | Value | Meaning |
  |---|---|---|---|
  | `ifc` | must | an object name with the prefix `ifc` | The interface of that subcircuit |
  | `ckt` | must | an object name with the prefix `ckt` | The circuit used |
  | `requirements` | may | a mapping from full metric names (`<name>.<id>`, see `tb`) to `{min, max}` | What this design needs of it |

  The key is the subcircuit's name in the netlist: `[A-Za-z_][A-Za-z0-9_]{0,63}`.
- **`specifications`:** free keys. `process` is a short name, such as `sky130`, of the form `[a-z][a-z0-9_-]{0,39}` (3.5). `vdd`, `temp` and each entry of `pins` are a number or a mapping of `min`, `nom`, `max`, `unit` and `description`.
- **`origin`:** `repo` (a repository such as `owner/name`), `commit` (40 hexadecimal digits), `path`, `license` and `authors` must be written; `tag` and `files` (a mapping from paths to `{sha256, size}`) may be. `license` is `{spdx, file}`: the SPDX licence expression of the original repository, and the path of its licence file there.
- **Changing a submodule** gives a new circuit at every level above it (7.5). A circuit made only to run one simulation, such as one with an ideal model in place of a submodule, still writes `base`, but takes a `name` of its own, such as `sky130-razavi-ideal`: it starts a series of its own and is never taken for a newer version of the original (3.4, 7.5).

## Files

| File | Requirement | Content |
|---|---|---|
| `toa.yaml` | must | The manifest |
| the files listed in `files` | must | The design |
| `README.md` | should | What the design is, how it was made, what is known of it |
| `LICENSE…` | when `license` has a `LicenseRef-` | The text of the licence (3.2.2) |

- A file may start with a licence header, `SPDX-License-Identifier: <expression>`, in a comment. When it does, every licence it names must be one named in `license`.
- **Source lists** follow circuits further, through `ifc` and `uses` (5.2).

## Checks at sealing

1. Every file listed in `files` exists in the object, and no path is listed twice.
2. When the objects referred to can be found: `ifc` is an interface; for every item of `uses`, `ckt` is a circuit whose `ifc` is the item's `ifc`.
3. Netlists, Verilog-A and RTL keep the external files outside the object (3.5):
   - they hold no model files of a process and no model cards (such as `.model` blocks with parameters of a process);
   - they include no library of a process;
   - every `include`, `.lib` and `ahdl_include` names a relative path that stays within the object, without `..`, absolute paths or environment variables, and points to a file listed in `files`;
   - they hold no local paths, host names, user names or strings shaped like keys;
   - the type of a device that drawing needs is written in an RNL `@DEVICE` record of the header record, such as `@DEVICE(model=…, terminals=…, type=…)`, not as a `.model`, not even one without parameters such as `.model nch nmos`.
4. A tool that can read RNL checks the netlists with it.

The external files the circuit needs are declared in `specifications.process` and `external`, and the user's local file map supplies them when it is assembled (7.6).

## Readable name

`ckt/<interface name>/<name>.g<generation>/`, for example `ckt/strongarm-comp/sky130-razavi.g1/ckt_…/` (6.2). The generation is the one in its `history.json` (3.4).

The readable name without the type directory and the slashes at its ends, `<interface name>/<name>.g<generation>` such as `strongarm-comp/sky130-razavi.g1`, is the `label` a tool should write for the circuit in a source list (5.1), and the name it should show for it. `<interface name>` is the `name` of the interface in `ifc`; when that cannot be found, the first 12 characters of the object name in `ifc` stand for it.

## Example

```yaml
type: ckt
license: CERN-OHL-P-2.0
ifc: ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5
name: wrap-lee
form: composition
author: Alice Zhang
description: StrongARM comparator built from Lee's dynamic latch.
base:
- ckt_cd15342bb4ed1cc1137937195f7cf3c3b8b03af5
uses:
  dyn_latch_cmp:
    ifc: ifc_6c2f0e9a8b7d6c5e4f3a2b1c0d9e8f7a6b5c4d3e
    ckt: ckt_f1ede73af6ba50ae0621e34b2b6f6b946dbf1ff1
files:
  netlist:
  - strongarm_comp.scs
```
