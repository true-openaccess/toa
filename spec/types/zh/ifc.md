<!-- translated from en/ifc.md sha256:8dbdeffcb5ad -->
# 类型规格 `ifc`：模块定义

模块定义说明一个模块是什么：它的名字、它做什么、它的引脚，就像子电路的头。它没有工艺，也没有指标：那些属于它的电路网表和测试用例。前缀是 `ifc`。本规格需要 TOA `0.1` 或更新版本的公共规则（第 2 到 8 章）；它的字段定义是 `types/ifc.schema.json`。

## 字段

除公共字段（3.2.1）外：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `type` | 必须 | `ifc` | 类型 |
| `name` | 必须 | `[a-z][a-z0-9]{0,39}(-[a-z0-9]+)*` | 模块定义的短名，例如 `strongarm-comp`。名字不必唯一：两个模块定义可以同名 |
| `title` | 必须 | 不超过 200 个字符的字符串 | 一行标题。别的类型可以不写（3.2.1） |
| `pins` | 必须 | 一个或多个引脚的列表 | 它的引脚，按子电路端口的顺序 |
| `probes` | 可以 | `{name, description}` 的列表 | 测试用例可以观测的模块内部结点（见下） |

每个引脚：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `name` | 必须 | `[a-zA-Z][a-zA-Z0-9_]{0,31}` | 端口名 |
| `kind` | 可以 | `analog`、`digital`、`supply` 或 `ground` | 这个引脚上是什么。没写的当普通信号画 |
| `direction` | 可以 | `in`、`out` 或 `inout` | 方向 |
| `width` | 可以 | 从 1 起的整数 | 总线宽度；不写就是 1（见下） |
| `description` | 可以 | 字符串 | 它做什么用 |

- **总线：** `width` 为 N 的引脚，代表子电路里前后相接的 N 个端口，按网表里的顺序。这些端口在网表里叫什么，例如 `LSB<5>` 或 `LSB_5`，各是哪一位，本标准都不规定：工具按位置对引脚和端口，从不按名字对。导出模块定义的工具应当把总线写成一个带 `width` 的引脚，不要每一位写一个。
- **探针** 是测试用例可以观测的模块内部结点，例如锁存器的两个结点：`name` 的写法和引脚名一样，`description` 必须写，说明这是什么结点。探针和引脚一样，是这个模块对外的约定：这个模块定义的每个电路网表都应当有对应的网。是哪个网，模块定义和电路网表都不写：由验证记录在每次运行时写明（`evd`），测试用例只写探针名（`tb`）。
- 模块定义不写工艺，也不声明外部文件（3.5）。
- `description`、`tags` 和 `base` 和任何类型一样可以写。

## 文件

| 文件 | 要求 | 内容 |
|---|---|---|
| `toa.yaml` | 必须 | 入口文件 |
| `symbol.rnl` | 应当 | 这个模块定义的 RNL `0.4` 符号：一个名为 `<name>-sym` 的包，画出这个模块；由使用它的网表把它用在子电路上。画这个模块的电路网表和测试用例用这个包，不另抄一份 |
| `README.md` | 可以 | 写给人看的关于这个模块定义的更多内容 |
| 图片 | 可以 | `README.md` 里显示的图片，例如框图、时序图，用相对路径引用 |
| `LICENSE…` | `license` 里有 `LicenseRef-` 时 | 许可证全文（3.2.2） |

- 模块定义里不放别的文件。模型、网表和较长的文字放进电路网表、测试用例和文档。
- **用哪个包画一个子电路，由引用决定，不看包名。** 在电路网表里，它定义的子电路用 `ifc` 所指模块定义的 `symbol.rnl` 画，`uses` 每一项的子电路用这一项的 `ifc` 所指模块定义的画；在测试用例里，被测的模块用它的 `ifc` 所指模块定义的画。工具把这些文件作为这些名字的包交给 RNL。包名里放不进模块定义的名字，因为文件不能包含自己所在对象的哈希（3.2.1）。
- **两个模块定义可以有相同的 `name`**（见上），这时它们的包也同名。两者出现在同一棵展开树里时，RNL 只用其中一个包，所以工具应当给警告。这不让任何对象不合格。
- **符号随模块定义走。** 符号变了就是一个新的模块定义，指向旧模块定义的电路网表不跟着变。所以符号应当在模块定义定稿前画好。以后真要改，就做一个 `base` 指向旧模块定义的新模块定义；库可以给它标 `latest`（5.2），电路网表的作者愿意的话再逐个跟进（7.5）。
- **来源表** 会往下展开模块定义（5.2）；模块定义不引用任何对象，所以不会多出什么。

## 定稿时的检查

- 引脚名和探针名合在一起不重复，比较时不分大小写。
- 有 `symbol.rnl` 时，它的端子就是这些引脚；能读 RNL 的工具检查它。
- 除 `toa.yaml`、`symbol.rnl`、`README.md` 和 `LICENSE…` 之外，每个文件都是 `README.md` 引用的图片。
- 修正模块定义的文字是一条说明（4.1），不是一个新的模块定义。引脚、探针或符号变了才做新的模块定义。

## 可读名

`ifc/<name>/`，例如 `ifc/strongarm-comp/ifc_945cdda7…/`（6.2）。

可读名去掉类型目录和斜杠，即 `<name>`，例如 `strongarm-comp`，就是工具在来源表里给这个模块定义写的 `label`（5.1），也是工具显示它时用的名字。

## 例子

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
