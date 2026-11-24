<!-- translated from en/evd.md sha256:6cd3a4292a4c -->
# 类型规格 `evd`：验证记录

验证记录是某人考察一个设计得到的东西：考察的是哪个对象、怎么考察的、在什么条件下、得到了什么。前缀是 `evd`。它的 `kind` 说明是哪一种验证记录。`0.1` 版只定义一种，`sim`：一次仿真，即在一个电路网表上运行测试用例；以后的版本可以加别的种类，例如签核检查、硅片实测和表征。本规格需要 TOA `0.1` 或更新版本的公共规则（第 2 到 8 章）；它的字段定义是 `types/evd.schema.json`。

## 字段

除公共字段（3.2.1）外，每条验证记录都有：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `type` | 必须 | `evd` | 类型 |
| `kind` | 必须 | `[a-z][a-z0-9_-]{0,31}` | 验证记录的种类。`0.1` 只定义了 `sim` |

- **工具不认识的种类：** 工具照原样保留和显示这条验证记录，只按公共规则（第 2 到 8 章）和上面两个字段检查。别的种类由本规格以后的修订定义；这不改变 `sim` 是什么。

种类为 `sim` 的验证记录还有：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `dut` | 必须 | 对象名；`0.1` 里前缀为 `ckt` | 被测的对象。`0.1` 里是一个电路网表；以后的版本可以允许别的类型 |
| `tbs` | 必须 | 一个或多个前缀为 `tb` 的对象名的列表 | 运行的测试用例 |
| `conditions` | 必须 | 映射 | 实际运行的条件，分三格（见下） |
| `environment` | 必须 | 映射 | 环境（见下） |
| `metrics` | 必须 | 指标全名（`<name>.<id>`，见 `tb`）到数的映射 | 分析给出的每个指标的值。没得到的指标不写 |
| `evidence` | 可以 | `{path, kind}` 的列表，可以加 `label` | `evidence/` 下的文件（见下） |

- **测的就是 `dut` 原样的展开树。** 验证记录不能拿别的电路网表代替树里的某个电路网表；要测某个子模块的另一个版本，先给它上面各层出新版（7.5），再测新版。
- **一条验证记录，一个条件点。** 扫描一组条件，就出多条验证记录。
- **`conditions`** 只放会随环境变化的东西；展开树、网表和分析由 `lock.json` 固定：
  - `parameters`：测试用例的 `parameters` 的取值，每个是单个数、字符串或布尔值，不能是列表；
  - `options`：仿真器和求解器的设置，例如 `temp`、`reltol` 或 `errpreset`；
  - `models`：所用模型文件的列表，每项有 `file`（不含 `/` 或 `\` 的文件名），可以有 `section`（一个名字或列表）、`version`、`match`（文件里必须都出现的字符串）、`process` 和 `sha256`（3.5）。
- **`environment`：** 必须写 `simulator`，带 `name` 和 `version`，可以带 `command`：运行它的整条命令行，选项也在里面。可以写 `pdk`（`process`、`release`）、`analysis`（`python`、`packages`）、`system`（`os`、`cpu`、`threads`）和 `note`。`environment` 里不含本机路径、主机名或用户名。
- **`evidence`：** 文件的 `kind` 是 `spectrum`、`curve`、`waveform`、`table` 或 `json`。它不是验证记录本身的 `kind`。
- **`description`**（3.2.1）写读者还应当知道的事，例如某个指标为什么没得到。

## 文件 `lock.json`

种类为 `sim` 的验证记录，顶层都有一个 `lock.json`（3.3）。它按名字记下仿真的那棵树，连同所用文件的 SHA-256，让没有 TOA 工具也能核对这条验证记录。它是一个 JSON 对象：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `toa` | 必须 | `"lock/1"` | 格式 |
| `dut` | 必须 | 对象名 | 和入口文件里的 `dut` 相同 |
| `tbs` | 必须 | `{object, files}` 的列表 | `tbs` 里的每个测试用例，顺序相同 |
| `tree` | 必须 | `{path, object, ifc, files}` 的列表 | `dut` 的展开树里的每个电路网表，按 `path` 排序 |

- `path` 是从被测电路网表到这个实例一路上的子电路名，用 `.` 连起来，例如 `core` 或 `core.latch`；被测电路网表的路径是 `""`。
- `object` 和 `ifc` 是对象名：电路网表和它的模块定义。`files` 把对象里每个文件的路径映射到它的 SHA-256，写成 64 位小写十六进制数：电路网表是它 `files` 里列出的文件；测试用例是它 `files` 里列出的文件、`analyze` 的 `entry` 和它的 `requirements`。
- 任何成员都可以另带一个给人看的 `label`，工具不得解读它。
- `lock.json` 里的名字是引用（3.3），所以验证记录的名字固定了整棵树。

## 文件

| 文件 | 要求 | 内容 |
|---|---|---|
| `toa.yaml` | 必须 | 入口文件 |
| `lock.json` | `sim` 必须 | 仿真的那棵树（见上） |
| `evidence/` | 可以 | 关键数据，例如一张图的数据。不是原始仿真数据 |
| `README.md` | 可以 | 读者应当知道的事 |

- **来源表** 不往下展开验证记录（5.2）。验证记录旁边的来源表列出验证记录引用的对象；从别的对象引到的验证记录不再多带什么。

## 定稿时的检查

种类为 `sim` 的验证记录：

1. `lock.json` 合上面的格式，它的 `dut` 和测试用例和入口文件一致。
2. `evidence` 的每个 `path` 都是对象里 `evidence/` 下的文件。
3. 找得到测试用例时，`metrics` 的每个键都是其中某个测试用例的某个指标的 `<name>.<id>`。
4. 找得到对象时：`dut` 是一个电路网表；每个测试用例测的是 `dut` 的模块定义；`tree` 就是 `dut` 的展开树，每个文件哈希都一致。
5. 一棵展开树里，每个模块定义只有一个电路网表：同一个模块定义的两个电路网表是同一个子电路的两份定义，仿真会拒绝。

## 可读名

`evd/<interface name>/<record label>/`，标签是 `<UTC time>-<short name>`，例如 `evd/strongarm-comp/20261103T1200Z-wrap-off/evd_…/`（6.2）。

可读名去掉类型目录和两头的斜杠，例如 `strongarm-comp/20261103T1200Z-wrap-off`，就是工具在来源表里给这条验证记录写的 `label`（5.1），也是工具显示它时用的名字。`<interface name>` 是 `dut` 所指对象的模块定义的 `name`；找不到时，用 `dut` 里对象名的前 12 个字符代替。验证记录的标签不在入口文件里：建验证记录的工具取建立时的 UTC 时间，写成 `YYYYMMDDTHHMMZ`，再向使用者要一个短名。显示别人建的验证记录时，工具用它在来源表里那一行的 `label`，没有就用 `<interface name>/` 加验证记录名的前 12 个字符。

## 例子

```yaml
type: evd
license: MIT
kind: sim
dut: ckt_1ec6249c0d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a
tbs:
- tb_ad96a1470e2d4c6b8a9f1e3d5c7b9a0e2f4d6c8b
author: Alice Zhang
conditions:
  parameters: {vdd: 1.8}
  options: {temp: 27}
  models:
  - {file: sky130.lib.spice, section: tt, version: '0.20.1'}
environment:
  simulator: {name: spectre, version: '23.1.0', command: spectre tb.scs +aps}
  analysis: {python: '3.11.9'}
metrics:
  tb_offset.offset: 1.7
```
