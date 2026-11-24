<!-- translated from en/ckt.md sha256:3c12eb1f6a08 -->
# 类型规格 `ckt`：电路网表

电路网表是一个模块定义的一种设计的一个版本：一份网表，连同它需要的文件。它引用自己所实现的模块定义，并通过 `uses` 引用它所用的其他模块定义的电路网表。前缀是 `ckt`。本规格需要 TOA `0.1` 或更新版本的公共规则（第 2 到 8 章）；它的字段定义是 `types/ckt.schema.json`。

## 字段

除公共字段（3.2.1）外：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `type` | 必须 | `ckt` | 类型 |
| `ifc` | 必须 | 前缀为 `ifc` 的对象名 | 它实现的模块定义 |
| `name` | 必须 | `[a-z][a-z0-9_-]{1,39}` | 这个设计的名字，例如 `sky130-razavi`。它的各代同名，这个名字也标出它们是哪一支（3.4） |
| `form` | 必须 | `[a-z][a-z0-9_-]{1,31}` | 这个电路网表本身是怎么写的（见下） |
| `files` | 必须 | 映射 | 按用途列出它的文件（见下） |
| `external` | 可以 | 列表 | 它需要、但不随它发布的别的外部文件（3.5） |
| `uses` | 可以 | 映射 | 它所用的其他模块定义的电路网表（见下） |
| `parameters` | 可以 | 名字到数、字符串或布尔值的映射 | 它的子电路的参数和默认值 |
| `specifications` | 可以 | 映射 | 作者在任何验证之前声明的情况：电源、温度、引脚范围、拓扑、目标 |
| `origin` | 可以 | 映射 | 从 TOA 之外的仓库取来的设计，写它从哪里取来（见下） |

- **`description`**（3.2.1）说这个设计是什么。
- **`form`** 按这个电路网表自己文件的内容判断，不看它用了什么：
  - `ideal`：用理想元件搭的模型；
  - `va`：Verilog-A 写的行为模型；
  - `rtl`：Verilog 或其他 RTL；
  - `transistor`：晶体管电路，即使它同时用了别的模块定义；
  - `passive`：只有工艺的无源器件，例如电阻串、电容阵列；
  - `gates`：门级网表，只有工艺标准单元的实例；
  - `composition`：只有其他模块的实例和它们之间的连线，没有器件，没有模型代码；
  - `mixed`：这一层本身有上面几种中的多种。

  可以用别的值；不认识某个值的工具照原样显示。
- **`specifications.process`**（3.5）在 `form` 是 `transistor`、`passive` 或 `gates`，或是带某个工艺的器件或单元的 `mixed` 时必须写。其他形态的电路网表可以写。同一棵展开树里的电路网表可以写不同的工艺。
- **`files`：** 每种用途是对象内文件路径的列表，相对于对象顶层，用 `/` 分隔：

  | 用途 | 要求 | 文件 |
  |---|---|---|
  | `netlist` | 必须，至少一个 | 网表；第一个定义这个模块的子电路 |
  | `va` | 可以 | Verilog-A |
  | `rtl` | 可以 | Verilog 和其他 RTL |
  | `packages` | 可以 | RNL 包 |
  | `extras` | 可以 | 网表需要的其他文件 |

- **`uses`** 把网表里实例化的子电路名映射到用于它的电路网表：

  | 字段 | 要求 | 取值 | 含义 |
  |---|---|---|---|
  | `ifc` | 必须 | 前缀为 `ifc` 的对象名 | 这个子电路的模块定义 |
  | `ckt` | 必须 | 前缀为 `ckt` 的对象名 | 所用的电路网表 |
  | `requirements` | 可以 | 指标全名（`<name>.<id>`，见 `tb`）到 `{min, max}` 的映射 | 这个设计对它的要求 |

  键是子电路在网表里的名字：`[A-Za-z_][A-Za-z0-9_]{0,63}`。
- **`specifications`：** 键自由。`process` 是一个短名字，例如 `sky130`，形如 `[a-z][a-z0-9_-]{0,39}`（3.5）。`vdd`、`temp` 和 `pins` 的每一项是一个数，或由 `min`、`nom`、`max`、`unit`、`description` 组成的映射。
- **`specifications.targets`** 是作者定的目标：从指标全名（`<name>.<id>`，见 `tb`）到一个目标或一串目标的映射。一个目标是 `{min, max, preset}`：`min` 和 `max` 至少写一个，可以写 `preset`，即那个测试用例里这个目标适用的预设名；不写 `preset` 时，对所有预设都适用。目标不是结果，结果在验证记录里。电路网表不会变，所以改目标就是出新的一代。
- **`origin`：** 必须写 `repo`（仓库，例如 `owner/name`）、`commit`（40 位十六进制数）、`path`、`license` 和 `authors`；可以写 `tag` 和 `files`（路径到 `{sha256, size}` 的映射）。`license` 是 `{spdx, file}`：原仓库的 SPDX 许可证表达式，和它在原仓库里的许可证文件路径。
- **换子模块**会让它上面每一层都出一个新的电路网表（7.5）。只为跑一次仿真做的电路网表，例如把某个子模块换成理想模型，照样写 `base`，但另起一个 `name`，例如 `sky130-razavi-ideal`：它自成一支，永远不会被当成原电路网表的新版（3.4、7.5）。

## 文件

| 文件 | 要求 | 内容 |
|---|---|---|
| `toa.yaml` | 必须 | 入口文件 |
| `files` 里列出的文件 | 必须 | 设计本身 |
| `README.md` | 应当 | 这个设计是什么、怎么做出来的、已知它的哪些情况 |
| `LICENSE…` | `license` 里有 `LicenseRef-` 时 | 许可证全文（3.2.2） |

- 文件可以用一行注释开头，写许可证文件头 `SPDX-License-Identifier: <expression>`。写了的话，其中每个许可证都必须是 `license` 里出现的。
- **来源表** 会往下展开电路网表，沿 `ifc` 和 `uses`（5.2）。

## 定稿时的检查

1. `files` 里列出的每个文件都在对象里，没有路径列两次。
2. 找得到所引用的对象时：`ifc` 是一个模块定义；`uses` 的每一项，`ckt` 是一个电路网表，它的 `ifc` 就是这一项的 `ifc`。
3. 网表、Verilog-A 和 RTL 把外部文件留在对象之外（3.5）：
   - 不含工艺的模型文件，不含模型卡（例如带工艺参数的 `.model` 块）；
   - 不包含工艺的库；
   - 每个 `include`、`.lib` 和 `ahdl_include` 写的是留在对象之内的相对路径，不用 `..`、绝对路径或环境变量，并指向 `files` 里列出的文件；
   - 不含本机路径、主机名、用户名或形如密钥的字符串；
   - 画图要用的器件类型，写在 RNL 头记录的 `@DEVICE` 里，例如 `@DEVICE(model=…, terminals=…, type=…)`；不为画图写 `.model`，像 `.model nch nmos` 这样不带参数的也不行。
4. 能读 RNL 的工具用 RNL 检查网表。

电路网表需要的外部文件在 `specifications.process` 和 `external` 里声明，组装时由使用者的本机映射表提供（7.6）。

## 可读名

`ckt/<interface name>/<name>.g<generation>/`，例如 `ckt/strongarm-comp/sky130-razavi.g1/ckt_…/`（6.2）。代数就是它的 `history.json` 里写的那个（3.4）。

可读名去掉类型目录和两头的斜杠，即 `<interface name>/<name>.g<generation>`，例如 `strongarm-comp/sky130-razavi.g1`，就是工具在来源表里给这个电路网表写的 `label`（5.1），也是工具显示它时用的名字。`<interface name>` 是 `ifc` 所指模块定义的 `name`；找不到那个模块定义时，用 `ifc` 里对象名的前 12 个字符代替。

## 例子

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
specifications:
  targets:
    tb_offset.offset: {max: 2}
```
