# code-tools — 代码工具 MCP 插件(随包官方)

编码任务的一等工具面:**查(search)→ 读(read)→ 改(write/edit)**。
内核只留 system.exec 当万能底牌(审批+强杀);日常高频的查/读走免审批直通,
改文件走审批——审批经济学与权限分级见对话裁决(2026-09-03,「内置固化=1」方案)。

对标 ripgrep 的搜索体验:**rg 引擎以库的形式内嵌**(grep-searcher/grep-regex,
即 ripgrep 本体同款核心 crate),不要求宿主机装 grep/rg(Windows cmd 无 grep),
单 exe 零运行时依赖。同路线先例:VS Code 随软件自带 ripgrep。

## 工具面与权限分级

| 工具 | 干什么 | 审批(annotations → 宿主映射) |
|---|---|---|
| `search` | 内容正则/字面搜索,返回 文件+行号+命中行 | `readOnlyHint` → read-only 直通 |
| `read` | 读文本文件,带行号,offset/limit 分页,智能截断 | `readOnlyHint` → read-only 直通 |
| `write` | 写文件(新建或整文覆盖),自动建父目录 | `destructiveHint` → 审批 |
| `edit` | 精确字符串替换(old_string 唯一命中才动手;CRLF 自动兼容) | `destructiveHint` → 审批 |

宿主侧映射(M7 S3):`readOnlyHint` → read-only + not-required;
`destructiveHint` → external-side-effect + required(工具内部始终
`isError:false` + JSON `ok/error`,保证错误详情回喂给模型)。

## 沙箱

`allowed_roots`(白名单根目录,防逃逸):所有路径参数必须落在任一根内。
- 规则:绝对路径直接校验;相对路径挂到第一个根下;词法归一(`.`/`..`)
  后取最深已存在祖先 canonicalize,再按组件前缀比对(防 `BoenMind2` 同名前缀);
  Windows `\\?\` 前缀剥离(W8 坑)。
- 根目录必须已存在;无效根启动时剔除并告警(全无效 = 工具调用时统一报错
  提示配置,不拒启),修好配置「重载 MCP」即恢复。
- 根为空 = 工具一律报错并提示配置方法(权限显式化,ADR-0006;不做静默 cwd 兜底)。

## 配置

`--config <json>`(BoenMind 批准接入时传 `config/mcp-code_tools.json`),
按 mtime 热读不适用(根目录表启动时定,改根需「重载 MCP」重启插件代);

```json
{
  "allowed_roots": ["D:\\96_CoderWorld\\BoenMind"],
  "max_results": 80,
  "max_output_chars": 16000,
  "max_file_bytes": 1048576
}
```

设置页 config_schema(自描述声明):`allowed_roots`(string,分号分隔多根)、
`max_results`(range)、`max_output_chars`(range)、`max_file_bytes`(range)。

默认跳过目录:`.git` `node_modules` `target` `dist` `build`;隐藏目录/文件跳过;
超 `max_file_bytes` 的文件搜索时跳过(read 不受限,分页自己扛)。

## 协议与形态

MCP 2024-11-05,JSON-RPC over stdio(逐行),手写零 SDK(同 web-multisearch)。
`--self-describe` 紧凑单行声明(扫描发现→批准接入两段式)。能力名落宿主侧为
`mcp.code_tools.{search|read|write|edit}`。

## 已知边界(如实)

- 非UTF-8 文件:read 走 lossy(替换符),edit/write 拒绝(报错明示)。
- 搜索为顺序遍历(walkdir),不并行;个人工作区量级足够,大仓可后续加并行。
- 会话绑定工作区(ADR-0018 cwd 注入)尚未接入 MCP 执行面(BACKLOG 在案),
  现阶段根目录由配置显式给出。
