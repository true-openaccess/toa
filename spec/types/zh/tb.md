<!-- translated from en/tb.md sha256:2561df1459a4 -->
# 类型规格 `tb`：测试用例

测试用例是测量一个模块定义的一种方法的一个版本：一份网表、运行它的命令，以及把结果变成指标的分析脚本。任何人都可以给任何模块定义写测试用例，一个模块定义可以有很多测试用例。前缀是 `tb`。本规格需要 TOA `0.1` 或更新版本的公共规则（第 2 到 8 章）；它的字段定义是 `types/tb.schema.json`。

## 字段

除公共字段（3.2.1）外：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `type` | 必须 | `tb` | 类型 |
| `ifc` | 必须 | 前缀为 `ifc` 的对象名 | 它测量的模块定义 |
| `name` | 必须 | `tb_[a-z0-9_]{1,36}` | 这个测试用例的名字，例如 `tb_static`。它的各代同名，这个名字也标出它们是哪一支（3.4）。它是它的指标全名的第一部分 |
| `files` | 必须 | 映射 | 按用途列出它的文件，和电路网表（`ckt`）一样：`netlist` 至少列一个，第一个是运行的那份网表；可以写 `va`、`rtl`、`packages`、`extras` |
| `external` | 可以 | 列表 | 它需要、但不随它发布的外部文件（3.5）。测试用例不写工艺 |
| `run` | 必须 | 字符串 | 运行仿真的命令，供参考；使用者自己的环境可以不同 |
| `analyze` | 必须 | 映射 | 分析脚本：必须写 `entry` 和 `python`（它需要的 Python 版本，例如 `'>=3.10'`），可以写 `requirements` |
| `metrics` | 必须 | 一个或多个指标的列表 | 它测什么（见下） |
| `simulators` | 可以 | `{name, version}` 的列表，`version` 可以不写 | 作者用来跑过它的仿真器。只供参考：工具不得因为换了仿真器就拒绝运行 |
| `parameters` | 可以 | 映射 | 测试用例自己的网表声明的参数，例如激励幅度、负载、电源；不是被测电路网表的参数。每个是 `{unit, description}` |
| `presets` | 可以 | 映射 | 有名字的几组条件，分 `parameters`、`options`、`models` 三格，写法和验证记录一样，只是 `parameters` 的值可以是列表：一次扫描，每个点出一条验证记录 |

每个指标：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `id` | 必须 | `[a-zA-Z][a-zA-Z0-9_]{0,31}` | 它的名字。全名是 `<name>.<id>`，例如 `tb_static.inl` |
| `unit` | 必须 | 字符串 | 单位 |
| `better` | 必须 | `lower`、`higher` 或 `none` | 哪个方向更好 |
| `tolerance` | 必须 | `{abs}`、`{rel}` 或两者都有，数值 | 两个结果相差多少仍算一致 |
| `description` | 可以 | 字符串 | 怎么测的 |

- **全名的含义不变。** 在一个测试用例的各代里，指标名的含义永远不变；含义不同的指标换一个名字。设计的目标和电路网表 `uses` 里的要求，都用全名写。
- **`description`**（3.2.1）说这个测试用例测什么、怎么测。

## 文件

| 文件 | 要求 | 内容 |
|---|---|---|
| `toa.yaml` | 必须 | 入口文件 |
| `files` 里列出的文件 | 必须 | 网表和它们需要的文件 |
| `analyze` 的 `entry` | 必须 | 分析脚本 |
| `analyze` 的 `requirements` | 写了时 | 它需要的 Python 包，每个写明确切版本 |
| `README.md` | 应当 | 写给人看：测什么，怎么测 |

- **分析脚本**的调用方式是 `<entry> <results> <output>`。它从目录 `<results>` 读一组条件的仿真结果，在 `<output>` 里写一个文件 `metrics.json`（每个指标一个数，按 `id`）和一个目录 `evidence/`。
- **它是别人的代码**（第 11 章）。没有使用者同意，工具不得运行它；第一次运行某个版本之前，应当把它和它相对上一版的差别给使用者看；应当在单独的进程和目录里运行它。这不算隔离：没有审过的测试用例应当在容器或虚拟机里运行。
- 文件可以用许可证文件头开头，和电路网表（`ckt`）一样。
- **来源表** 会往下展开测试用例，沿 `ifc`（5.2）。

## 定稿时的检查

1. `files` 里列出的每个文件、`entry` 和写了时的 `requirements` 都在对象里，`files` 里没有路径列两次。
2. 各指标的 `id` 不重复。
3. 找得到 `ifc` 时，它是一个模块定义。
4. 网表遵守电路网表的网表规则（`ckt`，定稿时的检查，第 3 项）。

## 可读名

`tb/<interface name>/<name>.g<generation>/`，例如 `tb/strongarm-comp/tb_offset.g1/tb_…/`（6.2）。代数就是它的 `history.json` 里写的那个（3.4）。

可读名去掉类型目录和两头的斜杠，即 `<interface name>/<name>.g<generation>`，例如 `strongarm-comp/tb_offset.g1`，就是工具在来源表里给这个测试用例写的 `label`（5.1），也是工具显示它时用的名字。`<interface name>` 是 `ifc` 所指模块定义的 `name`；找不到那个模块定义时，用 `ifc` 里对象名的前 12 个字符代替。

## 例子

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
