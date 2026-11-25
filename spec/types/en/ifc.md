# Type specification `ifc`: interface

An interface says what a block is: its name, what it does and its pins, like the header of a subcircuit. It has no process and no metrics: those belong to its circuits and testbenches. The prefix is `ifc`. This specification needs the common rules of TOA `0.1` or later (chapters 2 to 8); the definition of its fields is `types/ifc.schema.json`.

## Fields

Besides the common fields (3.2.1):

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `type` | must | `ifc` | The type |
| `name` | must | `[a-z][a-z0-9]{0,39}(-[a-z0-9]+)*` | The short name of the interface, such as `strongarm-comp`. Names need not be unique: two interfaces may share one |
| `title` | must | a string of at most 200 characters | A title of one line. For other types it may be left out (3.2.1) |
| `pins` | must | a list of one or more pins | Its pins, in the order of the subcircuit's ports |
| `probes` | may | a list of `{name, description}` | Nodes inside the module that testbenches may observe (below) |

Each pin:

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `name` | must | `[a-zA-Z][a-zA-Z0-9_]{0,31}` | The name of the port |
| `kind` | may | `analog`, `digital`, `supply` or `ground` | What the pin carries. A pin without it is drawn as an ordinary signal |
| `direction` | may | `in`, `out` or `inout` | Its direction |
| `width` | may | an integer from 1 | The width of a bus; 1 when not written (below) |
| `description` | may | a string | What it is for |

- **A bus:** a pin of `width` N stands for N ports of the subcircuit that follow each other, in the order of the netlist. This standard does not say what the ports are called in a netlist, such as `LSB<5>` or `LSB_5`, nor which bit each is: tools match pins and ports by position, never by name. A tool that exports an interface should write a bus as one pin with `width`, not as one pin per bit.
- **Probes** are nodes inside the module that testbenches may observe, such as the two nodes of a latch: `name` is of the form of a pin name, and `description`, which must be written, says what the node is. Like the pins, they are part of what the module promises: every circuit of the interface should have a net for each. Which net it is, neither the interface nor the circuit says: an evidence record says it for each run (`evd`), and testbenches name only the probe (`tb`).
- An interface names no process and declares no external files (3.5).
- `description`, `tags` and `base` may be written as for any type.

## Files

| File | Requirement | Content |
|---|---|---|
| `toa.yaml` | must | The manifest |
| `history.json` | when `base` is written | Its lineage, written by sealing (3.4) |
| `symbol.rnl` | should | The symbol of the interface in RNL `0.4`: a package named `<name>-sym` that draws the block; the netlists that use it apply it to their subcircuits. Circuits and testbenches that draw this block use this package rather than a copy of it |
| `README.md` | may | More about the interface, for people |
| images | may | Images that `README.md` shows, such as a block diagram or a timing diagram, referred to by relative paths |
| `LICENSE…` | when `license` has a `LicenseRef-` | The text of the licence (3.2.2) |

- Besides these, an interface holds no other files. Models, netlists and longer texts belong in circuits, testbenches and documents.

- **Which package draws a subcircuit is decided by references, not by the name of the package.** In a circuit, the subcircuit it defines is drawn with the `symbol.rnl` of the interface in `ifc`, and the subcircuit of each item of `uses` with that of the item's `ifc`; in a testbench, the block it measures is drawn with that of its `ifc`. A tool gives these files to RNL as the packages of those names. The name of the package cannot hold the name of the interface, because a file cannot hold the hash of its own object (3.2.1).
- **Two interfaces may share a `name`** (above), and then their packages share a name too. When both occur in one expanded tree, RNL uses only one of the two packages, so a tool should warn of it. This does not make any object invalid.
- **The symbol goes with the interface.** A new symbol gives a new interface, and the circuits that refer to the old one do not change with it. So the symbol should be drawn before the interface is sealed. To change it later, make a new interface whose `base` is the old one; a library may mark it `latest` (5.2), and circuits follow it one by one, if their authors wish (7.5).
- **Source lists** follow interfaces further (5.2); an interface refers to nothing, so this adds nothing.

## Checks at sealing

- The names of pins and probes together are unique, compared without regard to case.
- When `symbol.rnl` is present, its terminals are the pins, and a tool that can read RNL checks it.
- Every file other than `toa.yaml`, `symbol.rnl`, `README.md` and `LICENSE…` is an image that `README.md` refers to.
- A correction to the text of an interface is a note (4.1), not a new interface. A new interface is made when its pins, its probes or its symbol change.

## Readable name

`ifc/<name>/`, for example `ifc/strongarm-comp/ifc_945cdda7…/` (6.2).

The readable name without the type directory and the slashes, `<name>` such as `strongarm-comp`, is the `label` a tool should write for the interface in a source list (5.1), and the name it should show for it.

## Example

```yaml
type: ifc
license: MIT
name: strongarm-comp
title: StrongARM latched comparator
description: >-
  Clocked comparator with a regenerative latch; output valid in the
  evaluation phase.
pins:
- {name: inp, kind: analog, direction: in}
- {name: inn, kind: analog, direction: in}
- {name: clk, kind: digital, direction: in}
- {name: outp, kind: digital, direction: out}
- {name: outn, kind: digital, direction: out}
- {name: vdd, kind: supply}
- {name: vss, kind: ground}
probes:
- {name: latch_p, description: Positive node of the regenerative latch}
- {name: latch_n, description: Negative node of the regenerative latch}
```
