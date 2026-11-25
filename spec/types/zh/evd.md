<!-- translated from en/evd.md sha256:b97fe255f889 -->
# 类型规格 `evd`：验证记录

验证记录是某人考察一个设计得到的东西：考察的是哪个对象、怎么考察的、在什么条件下、得到了什么。前缀是 `evd`。它的 `kind` 说明是哪一种验证记录。`0.1` 版定义两种：`sim`，一次仿真，即在一个电路网表上运行测试用例；`mc`，一次 Monte Carlo 仿真，即在电路网表统计偏差的许多样本上运行测试用例。以后的版本可以加别的种类，例如签核检查、硅片实测和表征。本规格需要 TOA `0.1` 或更新版本的公共规则（第 2 到 8 章）；它的字段定义是 `types/evd.schema.json`。

## 字段

除公共字段（3.2.1）外，每条验证记录都有：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `type` | 必须 | `evd` | 类型 |
| `kind` | 必须 | `[a-z][a-z0-9_-]{0,31}` | 验证记录的种类。`0.1` 定义了 `sim` 和 `mc` |

- **工具不认识的种类：** 工具照原样保留和显示这条验证记录，只按公共规则（第 2 到 8 章）和上面两个字段检查。别的种类由本规格以后的修订定义；这不改变 `sim` 和 `mc` 是什么。

种类为 `sim` 的验证记录还有：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `dut` | 必须 | 对象名；`0.1` 里前缀为 `ckt` | 被测的对象。`0.1` 里是一个电路网表；以后的版本可以允许别的类型 |
| `tbs` | 必须 | 一个或多个前缀为 `tb` 的对象名的列表 | 运行的测试用例 |
| `conditions` | 必须 | 映射 | 实际运行的条件，分三格（见下） |
| `probes` | 测试用例列了探针时 | 探针名到网的映射 | 这次运行里，每个探针是被测电路网表的哪个网（见下） |
| `environment` | 必须 | 映射 | 环境（见下） |
| `metrics` | 必须 | 指标全名（`<name>.<id>`，见 `tb`）到数的映射 | 分析给出的每个指标的值。没得到的指标不写 |
| `evidence` | 可以 | `{path, kind}` 的列表，可以加 `label` | `evidence/` 下的文件（见下） |

- **测的就是 `dut` 原样的展开树。** 验证记录不能拿别的电路网表代替树里的某个电路网表；要测某个子模块的另一个版本，先给它上面各层出新版（7.5），再测新版。
- **一条验证记录，一个条件点。** 扫描一组条件，就出多条验证记录。
- **`conditions`** 只放会随环境变化的东西；展开树、网表和分析由 `lock.json` 固定：
  - `parameters`：测试用例的 `parameters` 的取值，每个是单个数、字符串或布尔值，不能是列表；
  - `options`：仿真器和求解器的设置，例如 `temp`、`reltol` 或 `errpreset`；
  - `models`：所用模型文件的列表，每项有 `file`（不含 `/` 或 `\` 的文件名），可以有 `section`（一个名字或列表）、`version`、`match`（文件里必须都出现的字符串）、`process` 和 `sha256`（3.5）。
- **`probes`** 给 `tbs` 里各测试用例列出的每个探针，写明 `dut` 所指电路网表里的一个网：它的子电路里的网，例如 `LP`；或者更下层的网，写成一路往下的实例名加网名，用 `.` 连起来，例如 `xlatch.LP`。它只写这些探针，不多不少。和 `dut`、`conditions` 一样，它是这次运行的输入：同一个模块定义的另一个电路网表，可以给同样的测试用例另外的网。
- **`environment`：** 必须写 `simulator`，带 `name` 和 `version`，可以带 `command`：运行它的仿真器自己的命令行，选项也在里面。只负责启动仿真器的本机启动器，例如沙箱、作业调度或容器，不算在里面，也不写：它和本机路径一样，属于这台机器。可以写 `pdk`（`process`、`release`）、`analysis`（`python`、`packages`）、`system`（`os`、`cpu`、`threads`）和 `note`。`environment` 里不含本机路径、主机名或用户名。
- **`evidence`：** 文件的 `kind` 是 `spectrum`、`curve`、`waveform`、`table` 或 `json`。它不是验证记录本身的 `kind`。
- **`description`**（3.2.1）写读者还应当知道的事，例如某个指标为什么没得到。

## 种类 `mc`

种类为 `mc` 的验证记录是一次 Monte Carlo 仿真：在一组条件下，把它的测试用例在模型所描述的统计偏差的许多样本上运行。它有 `sim` 的字段，也遵守那些规则，只有下面两个字段不同：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `conditions` | 必须 | 映射 | 和 `sim` 一样，另加第四格 `statistics`，必须写（见下） |
| `metrics` | 必须 | 指标全名到 `{n, failed, mean, sigma, min, max}` 的映射 | 每个指标在各样本上的统计量（见下） |

- **`statistics`** 写明样本怎样抽取、怎样运行：

  | 字段 | 要求 | 取值 | 含义 |
  |---|---|---|---|
  | `method` | 必须 | `mismatch`、`process` 或 `both` | 抽取哪种偏差：器件之间的失配、工艺的偏差，或两者都有 |
  | `points` | 必须 | 整数，至少为 1 | 样本数 |
  | `sampling` | 必须 | `random`、`lds` 或 `lhs` | 样本怎样抽取：普通随机、低差异序列，或拉丁超立方采样 |
  | `seed` | 必须 | 1 到 2147483647 的整数 | 随机数的种子，写实际用的那个；不能留给默认值 |
  | `first` | 可以 | 整数，至少为 1；不写时为 1 | 第一个样本的序号。样本是从 `first` 到 `first + points − 1` 的那些 |
  | `run` | 必须 | `each` 或 `batch` | 怎样运行的（7.6）：`each`，每个样本一次仿真；`batch`，全部样本在一次仿真里 |

  - 用 `lhs` 时，样本分在 `points` 个分箱里，`first` 为 1。
  - 统计模型在 `models` 的模型文件里，和任何运行一样。
- **第 k 个样本** 是单独仿真这一个样本得到的结果，也就是 `first` 为 k、`points` 为 1、`run` 为 `each` 的记录里会写的结果。用 `batch` 跑出的结果和它在每个指标的容差之内一致，但不必逐位相同。
- **`metrics`** 写测试用例的每个指标，没有一个样本得到的指标也写。每个指标在这条记录的各样本上：
  - `n` 是得到了这个指标的样本数，`failed` 是没得到的样本数，所以 `n + failed` 就是 `points`；
  - `n` 至少为 1 时写 `mean`、`min` 和 `max`；`n` 至少为 2 时写 `sigma`，即以 `n − 1` 为除数的标准差；
  - 它们由工具从 `samples.csv` 算出，不由分析脚本算，这样各处的算法一致。取值为 0 或 1 的指标，例如判决，`mean` 就是得到 1 的样本所占的比例。
- **`evidence/samples.csv`** 必须在记录里，并在 `evidence` 里列出，种类为 `table`。它是 UTF-8 编码、行尾为 LF 的 CSV 文件：
  - 第一行是 `sample`，后面是各指标的全名，按 `tbs` 的顺序和各测试用例 `metrics` 的顺序；
  - 之后每个样本一行，从 `first` 起：先是它的序号，然后是各指标的值，这个样本没得到的指标留空。仿真失败的样本也有一行，所有值都空；
  - 值写成十进制数，读回来是同一个数。
- **不记良率。** 目标在电路网表里（`ckt` 的 `specifications.targets`）：谁都可以拿 `samples.csv` 对着目标算，目标改了也不用重跑。
- **分析脚本为每个样本写进 `evidence/` 的东西** 不保留；工具可以为个别样本保留，各放在 `evidence/samples/<k>/` 下。

### 比较两次 Monte Carlo 仿真

两条种类为 `mc` 的验证记录，`tbs` 相同，`conditions` 除了 `statistics` 里的 `first`、`points` 和 `run` 都相同时，按下面两种方式之一比较：

- **逐样本比，** 条件是两者还有相同的 `dut` 和 `probes`，仿真器的名字和版本相同，用 `lhs` 时 `points` 也相同。这时它们的网表由 `lock.json` 固定，逐字节相同，样本也就相同。两条记录都有的每个样本，逐个指标按该指标的容差比较（`tb`）；一边得到、另一边没得到的指标算不一致。比较的每个样本都一致时，两条记录互相复现。
- **比统计量，** 其余情况都这样比。换一个电路网表，样本就不同，哪怕是同一个电路重新提取，或者原理图对提取出的网表；仿真器换了版本也可能不同。对两边的 `n` 都至少为 2 的每个指标（下面用 1 和 2 分别表示两条记录，`t` 是把该指标的容差用在所比的两个值上得到的界限）：
  - 均值一致，当 `|mean₁ − mean₂| ≤ max(t, 3 × √(sigma₁²/n₁ + sigma₂²/n₂))`；
  - 标准差一致，当 `|sigma₁ − sigma₂| ≤ max(t, 3 × √(sigma₁²/(2(n₁ − 1)) + sigma₂²/(2(n₂ − 1))))`；
  - 在两边的 `samples.csv` 里取值全是 0 或 1 的指标，只比均值，按二项分布：当 `|mean₁ − mean₂| ≤ 3 × √(p(1 − p)(1/n₁ + 1/n₂))` 时一致，其中 `p = (n₁mean₁ + n₂mean₂)/(n₁ + n₂)`。

  每个指标都这样一致时，两条记录统计上一致。工具这样说明，不称之为复现。

## 文件 `lock.json`

种类为 `sim` 或 `mc` 的验证记录，顶层都有一个 `lock.json`（3.3）。它按名字记下仿真的那棵树，连同所用文件的 SHA-256，让没有 TOA 工具也能核对这条验证记录。它是一个 JSON 对象：

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
| `lock.json` | `sim` 和 `mc` 必须 | 仿真的那棵树（见上） |
| `evidence/` | 可以 | 关键数据，例如一张图的数据。不是原始仿真数据 |
| `README.md` | 可以 | 读者应当知道的事 |

- **来源表** 不往下展开验证记录（5.2）。验证记录旁边的来源表列出验证记录引用的对象；从别的对象引到的验证记录不再多带什么。

## 定稿时的检查

种类为 `sim` 或 `mc` 的验证记录：

1. `lock.json` 合上面的格式，它的 `dut` 和测试用例和入口文件一致。
2. `evidence` 的每个 `path` 都是对象里 `evidence/` 下的文件。
3. 找得到测试用例时，`metrics` 的每个键都是其中某个测试用例的某个指标的 `<name>.<id>`。
4. 找得到对象时：`dut` 是一个电路网表；每个测试用例测的是 `dut` 的模块定义；`tree` 就是 `dut` 的展开树，每个文件哈希都一致。
5. 一棵展开树里，每个模块定义只有一个电路网表：同一个模块定义的两个电路网表是同一个子电路的两份定义，仿真会拒绝。
6. 找得到测试用例时，`probes` 写的恰好是它们列出的探针。

种类为 `mc` 的验证记录，还要：

7. `statistics` 遵守上面的规则。
8. 每个指标的 `n + failed` 等于 `points`，`mean`、`sigma`、`min` 和 `max` 按 `n` 的要求写。
9. `evidence/samples.csv` 合上面的格式，每个样本一行，各列就是 `metrics` 里的指标；每个指标的 `n` 和 `failed` 和它一致。

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

同一个电路网表的一次 Monte Carlo 仿真：

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
