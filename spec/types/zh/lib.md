<!-- translated from en/lib.md sha256:fb6b4e8ecd0e -->
# 类型规格 `lib`：冻结库

冻结库是冻结成一个对象的一组对象：一个库在某一刻的样子，例如它的一次发布，或者从几个库里挑出的一组对象。它带一份来源表格式的成员表；它是对象，所以这份表永远不变。前缀是 `lib`。本规格需要 TOA `0.1` 或更新版本的公共规则（第 2 到 8 章）；它的字段定义是 `types/lib.schema.json`。

- 本标准里的"库"仍指放对象的 git 仓库（6.1）。冻结库是一个对象，和别的对象一样放在库里。

## 字段

除公共字段（3.2.1）外：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `type` | 必须 | `lib` | 类型 |
| `name` | 必须 | `[a-z][a-z0-9]{0,39}(-[a-z0-9]+)*` | 冻结库的名字，例如 `toa-official`。它的各次发布同名，这个名字也标出它们是哪一支（3.4） |

- **`title`**（3.2.1）说是哪一次发布，例如 `TOA official library, 2026-12`。
- **下一次发布** 写 `base` 指向上一次。库给最新的那次标 `latest`（5.2）。
- 冻结库不写工艺，也不声明外部文件（3.5）。

## 文件

| 文件 | 要求 | 内容 |
|---|---|---|
| `toa.yaml` | 必须 | 入口文件 |
| `members.source.jsonl` | 必须 | 成员表，一份来源表（第 5 章） |
| `README.md` | 可以 | 发布说明 |

## 成员表

`members.source.jsonl` 遵守 3.3 对任何对象里这个文件定的规则：

- 头是 `{"toa": "source/1"}`，不多不少。其余每一行都是对象行，没有包含行。
- 对象行可以有 `object`、`label`、`refs`、`license`、`group` 和 `sources`；不得有 `removed` 和 `latest`。来源只能是 `git` 来源，不能是 `local`。
- 它按发布分支上的来源表那样严格地读（5.1），但大小受对象的上限（3.1）约束，而不是来源表的上限：最多 10 MiB。一行仍最多 64 KiB。
- **每一行的 `object` 都是引用**（3.3），和别的引用一样必须找得到。缓存字段里的名字，例如 `refs` 里的，不是引用。

冻结库另有：

- **来源只是提示：** 地址失效了，工具就到别处找这个对象（7.3）。
- **应当列全：** 成员需要的对象（5.2）也应当各占一行，各带来源，这样取冻结库的人不用再到别处找。工具写冻结库时默认列全；读到没列全的表时应当告诉使用者。
- 冻结库可以是另一个冻结库的成员，例如由几部分组成的官方发布。文档可以在 `refs` 里引用它，例如发布说明。

## 用法

- **工作区清单可以直接包含成员表，** 用一行包含行，`include` 写它在库里的路径（5.1、8.1）。分组和可读名照任何来源表一样显示。
- **取一个冻结库，** 就是取它、它的成员和成员需要的对象。它旁边的来源表列出了这些对象（5.2）。
- **库的清单和冻结库不同：** 库的清单会变，对象会加进来、删掉、被标记；成员表永远不变，只说某一刻有哪些对象、到哪里找。
- 和 git 标签不同，冻结库是对象：定稿后不变，别的对象可以引用它，它可以从几个库里挑成员，也不依赖某一个仓库还在。
- **来源表** 会往下展开冻结库，沿 `members.source.jsonl` 里的对象（5.2）。

## 定稿时的检查

1. `members.source.jsonl` 存在，并遵守上面的规则。
2. 没有一个对象名出现在两行上。
3. 找得到成员时，工具应当告诉使用者成员需要、但表里没列的对象。

不管对象是什么类型，定稿都像替换 `toa.yaml` 和 `lock.json` 里的草稿名一样，替换 `members.source.jsonl` 里的草稿名（7.1）。

## 可读名

`lib/<name>.g<generation>/`，例如 `lib/toa-official.g3/lib_…/`（6.2）。代数就是它的 `history.json` 里写的那个（3.4）。冻结库没有模块名这一级。

可读名去掉类型目录和两头的斜杠，即 `<name>.g<generation>`，例如 `toa-official.g3`，就是工具在来源表里给这个冻结库写的 `label`（5.1），也是工具显示它时用的名字。

## 例子

```yaml
type: lib
license: CC0-1.0
name: toa-official
title: TOA official library, 2026-12
base:
- lib_3b9e5c1d7a2f4e6b8c0d9a1e3f5b7c9d2e4f6a8b
```

它的 `members.source.jsonl` 的头几行：

```text
{"toa": "source/1"}
{"object": "ckt_1ec6249c0d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a", "label": "strongarm-comp/wrap-lee.g2", "group": "strongarm-comp", "sources": [{"git": "https://example.org/toa-lib.git"}]}
{"object": "ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5", "label": "strongarm-comp", "group": "strongarm-comp", "sources": [{"git": "https://example.org/toa-lib.git"}]}
```
