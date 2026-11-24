<!-- translated from en/doc.md sha256:7594acd5643c -->
# 类型规格 `doc`：文档

文档是关于模块定义、电路网表、测试用例、验证记录或冻结库的文字：设计笔记、测试用例的使用说明、架构说明、参考资料。它的正文是对象的 `README.md`。前缀是 `doc`。本规格需要 TOA `0.1` 或更新版本的公共规则（第 2 到 8 章）；它的字段定义是 `types/doc.schema.json`。

## 字段

除公共字段（3.2.1）外：

| 字段 | 要求 | 取值 | 含义 |
|---|---|---|---|
| `type` | 必须 | `doc` | 类型 |
| `refs` | 可以 | 对象名的列表 | 它讲的对象。这些是引用（3.3） |
| `date` | 可以 | 日期，`'YYYY-MM-DD'`，加引号 | 作者给它的日期 |

- **`title`**（3.2.1）不写时，就是 `README.md` 的第一个标题。是哪一种文档，例如 `design-note` 或 `tb-guide`，可以写在 `tags` 里。
- 只是顺带提到、不要求找得到的对象，在 `README.md` 里提，不列进 `refs`（3.3）。
- 对另一个对象的修正是那个对象旁边的说明（4.1），不是文档。

## 文件

| 文件 | 要求 | 内容 |
|---|---|---|
| `toa.yaml` | 必须 | 入口文件 |
| `README.md` | 必须 | 正文，用 Markdown 写 |
| 其他文件 | 可以 | 正文用到的图片和数据，用相对路径引用 |

- 显示文档时，工具不得自行加载对象之外的资源（第 11 章）。
- **来源表** 不往下展开文档（5.2）。

## 定稿时的检查

1. `README.md` 存在。
2. `README.md` 的每个相对链接和图片都指向对象里的文件。

## 可读名

`doc/<interface name>/<short name>/`，例如 `doc/strongarm-comp/kickback-mitigation/doc_…/`（6.2）。

可读名去掉类型目录和两头的斜杠，例如 `strongarm-comp/kickback-mitigation`，就是工具在来源表里给这份文档写的 `label`（5.1），也是工具显示它时用的名字。`<interface name>` 是 `refs` 里第一个模块定义的 `name`；`refs` 里没有模块定义的文档没有这一级，例如 `doc/kickback-mitigation/doc_…/`。短名不在入口文件里：建文档的工具向使用者要。显示别人建的文档时，工具用它在来源表里那一行的 `label`，没有就用它的标题。

## 例子

```yaml
type: doc
license: CC-BY-4.0
title: Reducing kickback in StrongARM comparators
tags: [design-note, comparator]
refs:
- ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5
date: '2026-10-28'
```
