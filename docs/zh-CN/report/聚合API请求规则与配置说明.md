# 聚合 API 请求规则与配置说明

本文按当前源码实现整理聚合 API 的配置方式、请求转发规则、模型目录 V2 route 和余额查询规则。源码仍是最终准入标准，本文用于日常配置、排查和交接。

## 适用范围

聚合 API 是平台 Key 的一种上游来源。命中聚合来源时，请求会转发到第三方 API 供应商；混合轮转还可以在聚合来源与本地 OpenAI/Claude/Gemini 账号池之间故障转移。

适合场景：

- 使用 New API、One API、OpenAI-compatible、Anthropic-compatible、Gemini-compatible 这类外部供应商。
- 某些平台 Key 不想使用账号池，直接绑定一个供应商。
- 账号池优先，但账号池不可用时由聚合 API 兜底。
- 聚合 API 优先，但全部聚合 route 失败时由账号池兜底。
- 某个模型需要固定转发到某个供应商模型。

不适合场景：

- 需要 CodexManager 代理官方账号登录态、RT/AT 刷新、账号健康切换的请求。这类仍应走账号池。
- 需要在聚合 API 内做复杂协议转换的请求。纯聚合 API 路由当前是 passthrough，只做少量请求覆盖和鉴权注入。

## 核心概念

### 平台 Key

平台 Key 是客户端使用的入口密钥。每个 Key 可以配置：

- 协议类型：OpenAI-compatible、Claude native、Gemini native。
- 轮转策略：账号池、聚合 API、账号池优先混合轮转、聚合 API 优先混合轮转。
- 绑定模型、推理等级、服务层级等请求默认值。
- 可选绑定一个首选聚合 API。

### 聚合 API

聚合 API 是一个可转发的上游供应商记录，包含：

- 供应商类型：`codex`、`claude`、`gemini`、`compatible`、`responses`。
- 上游基础地址：例如 `https://api.openai.com` 或带供应商前缀的 `https://open.bigmodel.cn/api/anthropic`。
- 认证方式：API Key 或用户名密码。
- 自定义鉴权参数。
- 自定义 action path。
- 可选余额查询配置。

聚合 API 连接不维护对外平台模型，但可以缓存按需访问供应商 `/models` 得到的上游模型列表；发现结果只辅助填写 V2 route，不会自动创建平台模型。

### 模型目录 V2 route

V2 route 用于把平台模型绑定到具体聚合 API 来源和上游模型。

例如：

- 平台模型：`gpt-5.5`
- 来源：`aggregate_api`
- 来源 ID：`ag_xxx`
- 上游模型：`gpt-5.4-mini`

命中该平台模型时，Gateway 读取 `model_routes` 构建对应聚合候选，并把该候选请求 JSON body 中的 `model` 改写为 route 的上游模型。模型管理页可手工输入上游模型，也可点击刷新按钮按需访问供应商 `/models`。

## 请求入口

Web 网关会把以下请求代理到 service：

- `/v1`
- `/v1/{*path}`
- `/v1alpha/{*path}`
- `/v1beta/{*path}`
- Gemini internal generate/count 路径

Web 到 service 的 body 限制由环境变量控制：

| 配置 | 默认值 | 说明 |
| --- | --- | --- |
| `CODEXMANAGER_GATEWAY_PROXY_MAX_BODY_BYTES` | `0` | `0` 表示不限制；大于 0 时按字节限制 Web 网关代理请求体大小。 |

Service 侧还会按请求路径识别协议：

| 请求路径 | 协议 |
| --- | --- |
| `/v1/messages`、`/v1/messages/*`、`/v1/messages?*` | `anthropic_native` |
| `/v1/models/*:generateContent`、`/v1beta/models/*:generateContent`、`/v1alpha/models/*:generateContent` | `gemini_native` |
| `/v1/models/*:streamGenerateContent`、`/v1beta/models/*:streamGenerateContent`、`/v1alpha/models/*:streamGenerateContent` | `gemini_native` |
| `/v1/models/*:countTokens`、`/v1beta/models/*:countTokens`、`/v1alpha/models/*:countTokens` | `gemini_native` |
| 其他标准 `/v1/*` | `openai_compat` |

## 轮转策略

平台 Key 的 `rotationStrategy` 支持以下值：

| 规范值 | 别名 | 行为 |
| --- | --- | --- |
| `account_rotation` | `account`、`account_rotate`、`账号轮转` | 只走账号池。 |
| `aggregate_api_rotation` | `aggregateapi`、`aggregate_api`、`aggregateapirotation`、`聚合api`、`聚合api轮转` | 只走聚合 API。 |
| `hybrid_rotation` | `hybrid`、`mixed`、`mixed_rotation`、`混合轮转`、`账号优先聚合兜底` | 先走账号池；账号池耗尽或不可用后再走聚合 API。 |
| `hybrid_aggregate_first_rotation` | `hybrid_aggregate_first`、`aggregate_first_hybrid`、`聚合API优先混合轮转`、`聚合API优先账号兜底` | 先完整尝试聚合 API；全部失败且响应尚未开始时再走账号池。 |

全局账号候选策略由 `CODEXMANAGER_ROUTE_STRATEGY` 或设置页控制：

| 值 | 别名 | 行为边界 |
| --- | --- | --- |
| `ordered` | `order`、`priority`、`sequential` | 只控制一条账号池 route 内具体账号的顺序。 |
| `balanced` | `round_robin`、`round-robin`、`rr` | 只在一条账号池 route 内均衡具体账号。 |

默认策略是 `ordered`。这项全局设置不重排模型 route，也不重排聚合 API route。

## 候选源选择规则

聚合 API 候选按以下步骤筛选和排序：

1. 按请求协议映射供应商类型。
2. 只取人工 `status = active` 且 `autoDisabled = false` 的聚合 API；自动熔断只排除业务候选，
   不阻止管理页读取连接或刷新余额。
3. 只取供应商类型匹配的聚合 API。
4. 读取请求平台模型的 enabled V2 aggregate routes，并按 `priority DESC` 分成硬优先级层。
5. 同一优先级内按 route 的 `weight` 做平滑加权轮转；首选失败时先尝试同级其它 route，再进入低优先级。
6. 将每条 route 的 `sourceId` 解析为聚合 API 连接，并把各自的 `upstreamModel` 绑定到候选。同一个连接的多条不同模型 route 会作为不同尝试保留。
7. 如果平台 Key 显式绑定了 `aggregateApiId`，只在该 ID 同时存在匹配 V2 route 时把对应候选放到第一位。

route 的调度状态按“平台 Key + 平台模型 + 来源类型 + 优先级”隔离，服务重启后重新开始。聚合 API 连接自身的 `sort` 以及全局 `ordered` / `balanced` 都不参与这一步。

协议到供应商类型的映射：

| 协议 | 聚合 API providerType |
| --- | --- |
| `anthropic_native` | `claude` |
| `gemini_native` | `gemini` |
| 客户端 `/v1/responses` | `codex`、`responses` |
| 其他 | `codex` |

`providerType = compatible` 会同时进入 `codex` 和 `claude` 候选，但不会进入 `gemini` 候选。它适合共用同一 URL 和密钥、并原生提供 `/v1/responses`、`/v1/chat/completions` 与 `/v1/messages` 的聚合供应商。

如果没有可用候选，会返回类似：

- `aggregate api not found for provider codex`
- `model_unavailable: gpt-5.5`

## 聚合 API 字段说明

| 字段 | 必填 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `providerType` | 否 | `codex` | 供应商类型。 |
| `supplierName` | 是 | 无 | 供应商显示名。 |
| `sort` | 否 | `0` | 只控制聚合 API 管理页和选择列表的显示顺序，不参与真实请求选路。 |
| `url` | 否 | 按 providerType 默认 | 上游 base URL，只允许 `http` / `https`，尾部 `/` 会被移除。 |
| `status` | 否 | `active` | `active` 或 `disabled`。 |
| `authType` | 否 | `apikey` | `apikey` 或 `userpass`。 |
| `key` | API Key 鉴权时是 | 无 | 聚合 API 的上游密钥，单独存储。 |
| `username` / `password` | userpass 鉴权时是 | 无 | 用户名密码，序列化后作为 secret 存储。 |
| `authCustomEnabled` | 否 | 不变/关闭 | 是否启用自定义鉴权参数。 |
| `authParams` | 自定义鉴权时是 | 无 | JSON 对象，见下文。 |
| `actionCustomEnabled` | 否 | 不变/关闭 | 是否启用自定义请求路径。 |
| `action` | 否 | 原始请求路径 | 自定义 action path，只能是相对路径。 |
| `balanceQueryEnabled` | 否 | `false` | 是否启用余额查询。 |
| `balanceQueryTemplate` | 否 | `generic` | 余额查询模板。 |
| `balanceQueryBaseUrl` | 否 | 聚合 API `url` | 余额查询 base URL。 |
| `balanceQueryAccessToken` | 否 | provider secret | 余额查询专用 access token，单独存储。 |
| `balanceQueryUserId` | 否 | 无 | New API 查询时作为 `New-Api-User` header。 |
| `balanceQueryConfigJson` | custom 模板时是 | 无 | 自定义余额 JSON 配置。 |
| `compatibilityConfigJson` | Responses 类型时否 | `{}` | Responses 内置档案、声明式字段规则、静态请求头和模型发现配置。 |
| `autoToggleEnabled` | 否 | `false` | 是否启用该连接的按日额度自动熔断与次日恢复。 |
| `consecutiveFailures` | 响应只读 | `0` | 连续命中明确 daily quota 错误的独立业务请求数。 |
| `autoDisabled` | 响应只读 | `false` | 当前是否被系统自动熔断；与人工 `status` 分开。 |
| `autoDisabledAt` | 响应只读 | 无 | 上一次达到阈值并自动熔断的 Unix 时间戳。 |
| `autoDisabledReason` | 响应只读 | 无 | 上一次自动熔断原因；当前值为 `daily_quota_exceeded`。 |

### 管理页显示顺序

聚合 API 列表会显示每条连接的 `sort`，并提供上移、下移按钮。点击按钮时，前端按调整后的页面顺序生成一组顺序值，通过批量接口在同一个 SQLite 事务中保存；任一连接不存在、ID 重复或写入失败时，整组更新都会回滚，不会留下只移动一部分的状态。编辑连接时仍可直接输入 `sort`，用于需要精确编号的场景。

这里的顺序仅用于聚合 API 管理页和选择列表。它不会改变真实请求的主备顺序或流量比例；运行时聚合 route 的先后由模型 route 的 `priority` 和 `weight` 决定。

列表响应中的 `modelSlugs` 仅由 V2 routes 派生，用于展示哪些平台模型引用当前连接；创建和更新连接时不接受该字段作为模型配置。

### providerType

支持以下规范值和别名：

| 规范值 | 别名 |
| --- | --- |
| `codex` | `codex`、`openai`、`openai_compat`、`gpt` |
| `claude` | `claude`、`anthropic`、`anthropic_native`、`claude_code` |
| `gemini` | `gemini`、`gemini_native`、`google`、`google_ai`、`google_gemini` |
| `compatible` | `compatible` |
| `responses` | `responses`、`response`、`responses_api`、`openai_responses` |

默认 URL：

| providerType | 默认 URL |
| --- | --- |
| `codex` | `https://api.openai.com/v1` |
| `claude` | `https://api.anthropic.com/v1` |
| `gemini` | `https://generativelanguage.googleapis.com` |
| `compatible` | `https://api.openai.com/v1` |
| `responses` | `https://api.openai.com/v1` |

`compatible` 会按客户端当前请求路径和请求体原样选择协议，不触发 Codex 专用传输改写，也不会把 Responses 请求桥接成 Claude Messages。为确保不同协议都能保留各自路径，使用该类型时应关闭自定义 action。

注意：请求转发时会保留 `url` 的路径前缀，然后追加客户端原始路径或自定义 action。也就是说，如果 `url` 写成 `https://api.example.com/v1`，客户端又请求 `/v1/chat/completions`，最终会变成 `https://api.example.com/v1/v1/chat/completions`。生产配置建议：

- 通用 OpenAI-compatible 供应商：`url` 写根地址，例如 `https://api.example.com`。
- 供应商必须带固定前缀时：把前缀写进 `url`，把真实接口路径交给客户端原始路径或 action。
- 已经把 `url` 写到 `/v1` 的供应商：启用 action，并把 action 写成 `/chat/completions`、`/responses` 这类不重复 `/v1` 的路径。

### status

支持：

- 启用：`active`、`enabled`、`enable`
- 禁用：`disabled`、`disable`、`inactive`

只有人工状态为 `active` 才具备进入业务候选的前提；如果同时存在 `autoDisabled = true`，仍会被
业务路由排除，具体规则见下一节。

### 自动启停与按日额度熔断

自动启停是每个聚合 API 独立配置的保护功能，默认关闭。它只参与以下三种包含聚合来源的轮转：

- `aggregate_api_rotation`
- `hybrid_rotation`
- `hybrid_aggregate_first_rotation`

纯 `account_rotation` 不使用聚合 API，因此不会读取或修改聚合 API 的熔断计数。

#### 状态边界

- `status` 是人工启停状态，只能由人工操作修改。
- `autoToggleEnabled` 是是否允许系统自动计数、熔断和次日恢复的配置开关。
- `autoDisabled` 是当前自动熔断状态，不是人工开关，也不会覆盖 `status`。
- 业务请求只有在 `status = active` 且 `autoDisabled = false` 时才会选择该连接。
- 自动熔断后，混合轮转会继续尝试同级/低级聚合 route，并按既有策略回退账号池；route 的
  `priority`、`weight`、`sortOrder` 和聚合连接 `sort` 均不被改写。
- 管理页列表、读取 secret、连接测试和余额刷新不依赖业务候选过滤，因此仍可检查或充值已熔断连接。

#### 计入熔断的错误

只有上游明确表达“每日额度已耗尽”时才计数。HTTP 状态码本身永远不足以触发计数；可分析的响应
范围是 2xx 或 4xx（排除 408），并且 body/SSE 中还必须出现以下明确语义之一：

- 结构化错误的 `code`、`type`、`status` 或 `reason` 等于明确的周期额度耗尽代码；当前支持
  `DAILY_LIMIT_EXCEEDED`、`WEEKLY_LIMIT_EXCEEDED`、`MONTHLY_LIMIT_EXCEEDED` 及对应的
  `*_USAGE_LIMIT_EXCEEDED` 形式。大小写不敏感，空格或连字符会规范为下划线。
- `message` 或 `detail` 明确包含 `daily usage limit exceeded`、`weekly usage limit exceeded` 或
  `monthly usage limit exceeded`。
- 余额查询在最近 30 分钟内成功确认 `remaining <= 0`，同时上游返回通用
  `code=upstream_error / Upstream request failed`。零余额只作为该明确应用层失败的辅助证据，
  不会单独触发熔断。
- OpenAI Responses envelope 的 `error`、`response.error`、`response.status_details.error`，或 SSE
  根级/错误事件包含上述信号。
- Anthropic、Gemini 或兼容供应商的结构化错误，只要能从上述字段确认同一 daily quota 语义。

以下情况不计入：

- 只有 HTTP 429、普通并发限流或分钟级速率限制，没有明确 daily quota 信号。
- HTTP 408、5xx。
- DNS、TLS、连接中断、网络错误、总超时或流式空闲超时。
- WAF、鉴权、模型不存在、上下文过长、参数校验等其它上游错误。
- 正文中偶然出现相似单词，但不位于受支持错误字段、也没有明确错误提示。

#### 连续失败和阈值

- 阈值固定为连续 3 个独立业务请求。
- 一个业务请求内部即使对同一聚合 API 最多重试 4 次，也最多增加一次计数。
- 达到阈值后保存 `autoDisabled = true`、`autoDisabledAt` 和
  `autoDisabledReason = daily_quota_exceeded`，并立即从后续业务候选中排除。
- 任意成功请求会把 `consecutiveFailures` 清零。
- 未达到阈值的连续次数允许跨本地自然日保留；跨日恢复只针对已经 `autoDisabled` 的记录。
- 自动状态更新不会修改连接 `updatedAt`，因此不会扰乱现有列表和 route 排序。

#### 手动解除和次日恢复

- 管理页“解除熔断”只清零 `consecutiveFailures` 并把 `autoDisabled` 设为 false，不修改人工
  `status`，也不关闭 `autoToggleEnabled`。
- 为保留审计信息，解除熔断不会删除上一次 `autoDisabledAt` 和 `autoDisabledReason`。
- 关闭 `autoToggleEnabled` 会同时清除当前计数和自动熔断状态，但同样保留历史时间和原因。
- 服务按机器本地自然日惰性检查。上一自然日自动熔断、且人工 `status` 仍为 `active` 的连接会在
  第二天清除当前自动状态和计数。
- 人工 `status = disabled` 的连接不会被跨日恢复；人工停用始终优先。
- 如果熔断后已经充值，可以直接点击“解除熔断”立即重新参与业务轮转，不必等待第二天。

持久化字段由 `132_aggregate_api_auto_toggle.sql` 添加。旧数据库升级后默认关闭自动启停，旧记录
不会因为 migration 被自动熔断或自动启用。

## action 和 URL 拼接规则

聚合 API 最终上游地址由 `url` 和 action path 组成。

### 未启用 action

如果没有启用自定义 action，使用客户端原始请求路径：

```text
url=https://api.example.com
client path=/v1/chat/completions
final=https://api.example.com/v1/chat/completions
```

### 启用 action

如果配置了 action，则忽略客户端原始路径，固定使用 action：

```text
url=https://api.example.com
action=/v1/responses
final=https://api.example.com/v1/responses
```

action 规则：

- 只能是路径，不能是完整 URL。
- `responses` 会自动规范成 `/responses`。
- `action` 里可以带 query，例如 `/v1/messages?beta=true`。
- 空 action 等价于没有自定义 action。

### base URL 带路径前缀

base URL 的路径前缀会保留：

```text
url=https://open.bigmodel.cn/api/anthropic
action=/v1/messages
final=https://open.bigmodel.cn/api/anthropic/v1/messages
```

## 鉴权配置

### API Key 默认鉴权

当 `authType = apikey` 且未启用自定义鉴权时，转发时注入：

```http
Authorization: Bearer <key>
```

### API Key 自定义 header

```json
{
  "location": "header",
  "name": "x-api-key",
  "headerValueFormat": "raw"
}
```

字段说明：

| 字段 | 值 | 说明 |
| --- | --- | --- |
| `location` | `header` | 把密钥放到 header。 |
| `name` | 任意合法 header 名 | header 名必填。 |
| `headerValueFormat` | `bearer` 或 `raw` | `bearer` 会注入 `Bearer <key>`；`raw` 只注入原始 key。 |

示例：

```json
{
  "location": "header",
  "name": "Authorization",
  "headerValueFormat": "bearer"
}
```

```json
{
  "location": "header",
  "name": "api-key",
  "headerValueFormat": "raw"
}
```

### API Key 自定义 query

```json
{
  "location": "query",
  "name": "api_key"
}
```

转发时会把 key 写入最终 URL query：

```text
https://api.example.com/v1/chat/completions?api_key=<key>
```

如果 query 中已有同名参数，会先移除旧值再追加新值。

### 用户名密码默认鉴权

当 `authType = userpass` 且未启用自定义鉴权时，转发使用 HTTP Basic Auth。

### 用户名密码自定义 headerPair

```json
{
  "mode": "headerPair",
  "usernameName": "x-user",
  "passwordName": "x-password"
}
```

转发时注入：

```http
x-user: <username>
x-password: <password>
```

### 用户名密码自定义 queryPair

```json
{
  "mode": "queryPair",
  "usernameName": "username",
  "passwordName": "password"
}
```

转发时写入 query：

```text
?username=<username>&password=<password>
```

## 转发 header 规则

聚合 API 会透传大部分客户端 header，但以下 header 不会透传：

- `authorization`
- `x-api-key`
- `api-key`
- `content-length`
- `connection`
- `proxy-authorization`
- `proxy-authenticate`
- `te`
- `trailer`
- `transfer-encoding`
- `upgrade`
- `host`
- 自定义鉴权注入的 header 名

流式请求还会丢弃客户端 `accept`，改为：

```http
Accept: text/event-stream
```

这样可以避免客户端旧鉴权、错误 host、错误 content-length 或重复鉴权污染上游请求。

## 请求体处理规则

纯聚合 API 路由是 passthrough，默认不做账号池协议适配。

仍会执行的处理：

- 平台 Key 默认模型、推理等级、service tier 会写入请求。
- 非原生 Codex 客户端访问 `/v1/responses` 且没有显式 stream 时，会默认补 `stream=true`。
- 如果命中聚合 API V2 route，会使用该 route 的 `upstreamModel` 改写当前候选 JSON body 顶层 `model` 字段。
- `responses` 供应商只承接客户端 `/v1/responses`，要求上游直接输出标准 Responses JSON 或 SSE；网关不做响应协议转换。
- Responses 请求按“内置档案 < 聚合 API 配置 < 模型 route 覆盖”合并声明式兼容配置。
- 会执行文本输入长度检查。

不会做的处理：

- 不把 OpenAI chat 自动深度转换成官方 Codex Responses 账号池请求。
- 不使用账号池的 AT/RT、会话绑定、账号健康预检。
- 不使用账号池计费归属。

## 通用 Responses API 兼容框架

`providerType = responses` 用于原生支持 OpenAI Responses API 格式的第三方供应商。它只接收客户端 `/v1/responses`，上游必须直接返回标准 Responses 非流式 JSON 或 SSE 事件；当前实现不会把供应商私有响应转换成 Responses。

### 配置优先级

兼容配置按以下顺序合并，后者覆盖前者；对象字段使用递归合并：

1. 内置只读档案。
2. 聚合 API 的 `compatibilityConfigJson`。
3. 模型 V2 route 的 `compatibilityOverrideJson`。

内置档案名称：

| profile | 用途 |
| --- | --- |
| `openai_standard` | 标准 OpenAI Responses 行为。 |
| `deepseek` | DeepSeek Responses；当前会拒绝 thinking 模式不支持的 `tool_choice=required`，提示改用 `auto`。 |
| `generic_responses` | 其它标准 Responses 供应商的保守默认档案。 |

内置档案本身不可编辑。每条聚合 API 和每条模型 route 只保存自己的覆盖 JSON，因此以后新增 `deepseek-v4-pro` 时，通常只需新增或修改模型 route 的 `upstreamModel`，无需修改网关核心代码。只有 Pro 的请求字段约束与 Flash 不同，才需要在该 route 增加局部覆盖。

### 字段规则

`fieldPolicies`（兼容别名 `requestFields`）是以字段路径为键的对象。字段路径支持点号，例如 `reasoning.effort`、`text.verbosity`。

| strategy | 行为 |
| --- | --- |
| `pass` | 保持原值。 |
| `drop` | 字段存在时删除。 |
| `reject` | 字段存在时拒绝请求并返回明确错误。 |
| `replace` | 使用规则中的 `value` 替换；父对象不存在时会创建。 |
| `map` | 当前值为字符串时，使用规则 `value` 对象进行枚举映射。 |

示例：

```json
{
  "profile": "deepseek",
  "fieldPolicies": {
    "reasoning.effort": {
      "strategy": "map",
      "value": {
        "max": "high"
      }
    },
    "metadata.private": "drop",
    "tool_choice": "pass"
  }
}
```

字段删除、替换、映射和拒绝会写入网关日志，日志只记录字段路径和动作，不记录密钥或静态 header 值。

### 静态请求头

`staticHeaders` 支持字符串值和以下有限占位符：

- `${secret}`：当前聚合 API secret。
- `${model}`：route 解析后的上游模型。
- `${supplier}`：供应商显示名。

示例：

```json
{
  "staticHeaders": {
    "x-provider-model": "${model}",
    "x-provider-name": "${supplier}"
  }
}
```

禁止通过兼容配置覆盖 `Authorization`、`x-api-key`、`api-key`、`Content-Length`、`Host`、`Connection` 和 `Transfer-Encoding` 等受管 header。鉴权仍应使用聚合 API 的认证配置。

### 模型发现

模型管理页的刷新按钮调用 `aggregateApi/discoverModels`。成功后提供可选列表，失败时保留手工输入；如果以前成功发现过模型，刷新失败会返回旧缓存并显示缓存时间。

```json
{
  "modelDiscovery": {
    "path": "/models",
    "itemsPath": "data",
    "idPath": "id",
    "displayNamePath": "display_name",
    "pagination": {
      "enabled": true,
      "pageParam": "page",
      "cursorParam": "after",
      "limitParam": "limit",
      "pageSize": 100,
      "maxPages": 10,
      "hasMorePath": "has_more",
      "lastIdPath": "last_id"
    }
  }
}
```

`pagination.enabled` 默认是 `false`；启用后支持标准 `has_more` / `last_id` 游标，也会递增 `page`。单次刷新最多 100 页、10000 个去重模型。发现结果只填充 route 的上游模型选择，不自动创建对外平台模型。

### DeepSeek Flash 配置

聚合 API：

```json
{
  "providerType": "responses",
  "supplierName": "DeepSeek",
  "url": "https://api.deepseek.com",
  "authType": "apikey",
  "compatibilityConfigJson": {
    "profile": "deepseek",
    "modelDiscovery": {
      "path": "/models",
      "itemsPath": "data",
      "idPath": "id"
    }
  }
}
```

模型 route：

```json
{
  "sourceKind": "aggregate_api",
  "sourceId": "选择上面的聚合 API",
  "upstreamModel": "deepseek-v4-flash",
  "compatibilityOverrideJson": null
}
```

项目验证脚本和人工测试只从系统环境变量 `DEEPSEEK_API_KEY` 读取密钥，不应把真实 key 写入仓库、日志或示例配置。

### previous_response_id 供应商亲和

成功的标准 Responses 响应会持久化 `response.id` 与实际聚合 API 的绑定。后续请求带 `previous_response_id` 时只允许回到原聚合 API，并禁止跨供应商故障转移：

- 找到绑定且供应商可用：只请求该供应商，允许同一供应商内部重试。
- 绑定供应商当前不可用：返回冲突错误，不切换到其它供应商或账号池。
- 没有历史绑定但只有一个候选，或平台 Key 显式指定聚合 API：固定该候选。
- 没有历史绑定且存在多个候选：拒绝请求，避免把有状态上下文发给错误供应商。

## 重试与失败规则

每个聚合 API 候选最多会尝试 4 次：首次请求 + 3 次重试。

失败处理：

| 场景 | 行为 |
| --- | --- |
| 当前候选缺少 secret | 记录 403 失败，尝试下一个候选。 |
| URL 或 authParams 无效 | 记录失败，尝试下一个候选。 |
| 上游超时 | 记录 504 失败，尝试重试或下一个候选。 |
| 上游返回非 2xx | 摘要上游错误体，当前候选重试；最终对客户端按 502 处理。 |
| 非流式上游返回 2xx，但响应体读取失败或被截断 | 在交付客户端前识别为当前候选失败，继续下一个候选。 |
| 所有候选失败 | 返回最后一次失败信息。 |
| 没有候选 | 返回 404 或 `aggregate api not found...`。 |

非流式聚合成功响应会在交付客户端前完整缓冲。这样即使上游先返回 `2xx` 响应头、随后响应体断开，也仍可尝试下一条聚合 route；在 `hybrid_aggregate_first_rotation` 下，聚合候选最终耗尽后还可回退账号池。流式响应不能采用同样的完整缓冲，一旦已经向客户端开始输出，就不会再切换来源，避免把两个上游的响应拼接到同一个客户端请求中。

`hybrid_aggregate_first_rotation` 下，上述候选全部失败或没有可用聚合 route 时，只要客户端响应尚未开始，原始请求会交回账号池继续尝试。对于 Anthropic `/v1/messages/count_tokens` 和 Gemini `:countTokens`，聚合候选耗尽后会使用原始协议请求体恢复账号侧的本地 token 估算响应，不会把这些工具路径错误转发给账号上游。手工配置错误、验证码、登录墙或 WAF 不会被绕过。

请求日志会记录：

- trace id
- 原始路径和适配路径
- response adapter
- 平台 Key
- 实际来源类型 `aggregate_api`
- 实际来源 ID
- 聚合 API 供应商名和 URL
- 尝试过的聚合 API ID
- 上游模型
- 状态码、耗时、token、错误摘要

以上“尝试过的聚合 API ID”适用于最终由聚合链路结束的请求。聚合优先混合轮转在聚合候选
全部失败、随后由账号池结束请求时，当前最终请求日志只保留账号来源，不携带前置聚合尝试
ID；因此不能仅凭 `actual_source_kind = openai_account` 判断网关没有先尝试聚合 API。

## 余额查询配置

余额查询只影响管理界面展示，不参与实时请求转发。即使上一次查询得到 `remaining = 0`，本地也
不会提前过滤该连接；只要连接仍为 `active` 且存在启用的模型 route，网关仍会请求上游。
如果上游因余额或日额度耗尽返回错误，混合轮转才按策略尝试下一聚合候选或回退账号池。

启用字段：

```json
{
  "balanceQueryEnabled": true,
  "balanceQueryTemplate": "generic"
}
```

支持模板：

| 模板 | 别名 | 说明 |
| --- | --- | --- |
| `generic` | `generic` | 通用余额接口探测。 |
| `new_api` | `newapi`、`new_api` | New API 格式余额查询。 |
| `custom` | `custom`、`custom_json` | 自定义余额接口和 JSON 字段路径。 |

余额请求默认 header：

```http
Accept: application/json
Accept-Encoding: identity
User-Agent: codex-manager/aggregate-api-balance
```

### generic 模板

请求顺序：

1. `GET <base>/user/balance`
2. 如果 404、405、501、非 JSON 或缺少余额字段，则 fallback 到 `GET <usage_base>/v1/usage`

base URL 规则：

- 优先使用 `balanceQueryBaseUrl`。
- 未配置时使用聚合 API `url`。
- fallback `/v1/usage` 时，如果未显式配置 `balanceQueryBaseUrl` 且 `url` 以 `/v1` 结尾，会先去掉 `/v1`。

支持的余额字段：

- `remaining`
- `balance`
- `available`
- `quota.remaining`
- `data.remaining`
- `data.balance`
- `data.available`
- `data.quota.remaining`
- `credits.balance`

有效性字段：

- `success`
- `is_active`
- `active`
- `data.is_active`
- `data.active`
- `isValid`
- `is_valid`
- `data.isValid`
- `data.is_valid`
- `status` 不应为 `expired`、`quota_exhausted`、`disabled`

其他字段：

- 单位：`unit`、`currency`、`data.unit`、`data.currency`，默认 `USD`。
- 套餐：`planName`、`plan_name`、`mode`、`data.planName`、`data.plan_name`、`data.group`、`data.mode`。
- 总额：`total`、`quota.limit`、`data.total`、`data.quota.limit`。
- 已用：`used`、`used_quota`、`quota.used`、`data.used`、`data.used_quota`、`data.quota.used`。

### new_api 模板

请求：

```http
GET <base>/api/user/self
Authorization: Bearer <balanceQueryAccessToken 或 provider key>
New-Api-User: <balanceQueryUserId，可选>
```

base URL 规则：

- 优先使用 `balanceQueryBaseUrl`。
- 未配置时使用聚合 API `url`。
- 如果未配置 `balanceQueryBaseUrl` 且 `url` 以 `/v1` 结尾，会自动去掉 `/v1`。

字段换算：

- `data.quota / 500000` => remaining USD
- `data.used_quota / 500000` => used USD
- `remaining + used` => total USD
- `data.group` 或 `data.plan` => plan

### custom 模板

配置示例：

```json
{
  "method": "GET",
  "path": "/api/user/self",
  "auth": "balance_bearer",
  "remainingPath": "data.quota",
  "unit": "USD",
  "multiplier": 0.000002,
  "totalPath": "data.total",
  "usedPath": "data.used_quota",
  "planPath": "data.group",
  "validPath": "success",
  "invalidMessagePath": "message"
}
```

字段说明：

| 字段 | 必填 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `method` | 否 | `GET` | 只支持 `GET`、`POST`。 |
| `path` | 是 | 无 | 相对路径，不能是完整 URL。 |
| `auth` | 否 | `provider_bearer` | `provider_bearer`、`balance_bearer`、`none`。 |
| `remainingPath` | 是 | 无 | 剩余额度 JSON 路径。 |
| `unit` | 否 | `USD` | 展示单位，最长 16 字符。 |
| `multiplier` | 否 | `1` | 数值换算倍率，必须大于 0。 |
| `totalPath` | 否 | 无 | 总额度 JSON 路径。 |
| `usedPath` | 否 | 无 | 已用额度 JSON 路径。 |
| `planPath` | 否 | 无 | 套餐名 JSON 路径。 |
| `validPath` | 否 | 无 | 可用状态 JSON 路径。 |
| `invalidMessagePath` | 否 | 无 | 不可用原因 JSON 路径。 |

`auth` 说明：

| auth | 行为 |
| --- | --- |
| `provider_bearer` | 使用聚合 API 主 key 作为 Bearer。 |
| `balance_bearer` | 优先使用 `balanceQueryAccessToken`，没有则回退到主 key。 |
| `none` | 不注入 Authorization。 |

JSON 路径使用点号，例如：

- `data.remaining`
- `data.items.0.balance`

## 常见配置示例

### OpenAI-compatible 供应商

```json
{
  "providerType": "codex",
  "supplierName": "OpenAI Compatible",
  "sort": 10,
  "url": "https://api.example.com",
  "authType": "apikey",
  "key": "sk-xxx",
  "status": "active"
}
```

客户端请求：

```text
POST /v1/chat/completions
```

最终上游：

```text
POST https://api.example.com/v1/chat/completions
Authorization: Bearer sk-xxx
```

### New API 供应商

```json
{
  "providerType": "codex",
  "supplierName": "New API",
  "url": "https://newapi.example.com",
  "authType": "apikey",
  "key": "sk-xxx",
  "balanceQueryEnabled": true,
  "balanceQueryTemplate": "new_api",
  "balanceQueryAccessToken": "admin-token-or-user-token"
}
```

余额查询会访问：

```text
GET https://newapi.example.com/api/user/self
```

### Anthropic-compatible 供应商

```json
{
  "providerType": "claude",
  "supplierName": "Claude Compatible",
  "url": "https://api.anthropic.com/v1",
  "authType": "apikey",
  "key": "sk-ant-xxx",
  "authCustomEnabled": true,
  "authParams": {
    "location": "header",
    "name": "x-api-key",
    "headerValueFormat": "raw"
  }
}
```

客户端请求：

```text
POST /v1/messages
```

最终上游：

```text
POST https://api.anthropic.com/v1/messages
x-api-key: sk-ant-xxx
```

### 带路径前缀的 Claude 供应商

```json
{
  "providerType": "claude",
  "supplierName": "Claude Proxy",
  "url": "https://open.bigmodel.cn/api/anthropic",
  "authType": "apikey",
  "key": "xxx",
  "actionCustomEnabled": true,
  "action": "/v1/messages"
}
```

最终上游：

```text
https://open.bigmodel.cn/api/anthropic/v1/messages
```

### Gemini 供应商

```json
{
  "providerType": "gemini",
  "supplierName": "Gemini",
  "url": "https://generativelanguage.googleapis.com",
  "authType": "apikey",
  "key": "AIza..."
}
```

Gemini 模型同样在模型目录 V2 中新增并配置 route；上游模型名可手工填写，也可在供应商提供兼容 `/models` 时使用按需发现。

## 模型目录 V2 与聚合 route

聚合 API 不再维护供应商模型模板、模型池或来源映射。模型和 route 都由模型目录 V2 管理：

这里的 V2 是 CodexManager 受管目录：`aggregate_api_rotation` 和两种混合轮转使用它；纯
`account_rotation` 的模型列表与请求遵循 OpenAI 官方账号池目录。桌面端和 Web 端都不会
写入或下载 `~/.codex/models_cache.json`，受管目录也不会覆盖 Codex 官方缓存。

1. 在模型管理页新增或编辑平台模型。
2. 添加 `sourceKind=aggregate_api` 的 route。
3. 选择聚合 API 的 source ID，手工填写供应商真实 `upstreamModel`，或点击刷新按钮读取发现列表。
4. 一次保存原子提交 model、price tiers、routes、permission groups 和 instructions policy。

运行规则：

- 启动、连接编辑、route 测试和真实请求不会自动访问供应商 `/models`；只有模型管理页点击刷新按钮时才访问。
- 如果平台模型没有 enabled route，会返回 `model_unavailable: <model>`。
- 候选源只保留 enabled route 引用的 active 聚合 API。
- 每个候选独立使用自己的 route `upstreamModel`，请求体不会在候选间泄漏。
- route 可保存 `compatibilityOverrideJson`，只覆盖该模型到该供应商的 Responses 行为。
- route 的 `priority` 越大越优先；同优先级用 `weight` 做平滑加权，当前优先级全部失败后才进入低优先级。
- route 的 `sortOrder` 只控制页面显示，允许负数、`0` 和重复值，不参与真实请求。
- 聚合连接的 `sort` 也只控制管理页和下拉列表显示；不要用它配置主备请求顺序。
- 连接测试从引用当前聚合 API 的 enabled V2 routes 中选择具体模型，不做发现或导入。

## 管理接口

桌面端通过 Tauri command 调 service RPC：

| 功能 | Tauri command | RPC method |
| --- | --- | --- |
| 列表 | `service_aggregate_api_list` | `aggregateApi/list` |
| 创建 | `service_aggregate_api_create` | `aggregateApi/create` |
| 更新 | `service_aggregate_api_update` | `aggregateApi/update` |
| 批量更新显示顺序 | `service_aggregate_api_update_sorts` | `aggregateApi/updateSorts` |
| 解除自动熔断 | `service_aggregate_api_recover` | `aggregateApi/recover` |
| 读取 secret | `service_aggregate_api_read_secret` | `aggregateApi/readSecret` |
| 删除 | `service_aggregate_api_delete` | `aggregateApi/delete` |
| 测试连接 | `service_aggregate_api_test_connection` | `aggregateApi/testConnection` |
| 发现模型 | `service_aggregate_api_discover_models` | `aggregateApi/discoverModels` |
| 刷新余额 | `service_aggregate_api_refresh_balance` | `aggregateApi/refreshBalance` |

模型目录 V2 使用独立的 `service_managed_model_*_v2` 命令和 `apikey/managedModel*V2` RPC；模型发现 RPC 只返回并缓存供应商模型，不创建平台模型或导入模板。

前端 API 封装在：

```text
apps/src/lib/api/account-client.ts
```

桌面端调用必须走 `invoke` / `invokeFirst` 和 `withAddr()`，不要直接 `fetch()` service。

## 排障清单

### 请求没有走到聚合 API

检查：

1. 平台 Key 的 `rotationStrategy` 是否是 `aggregate_api_rotation`、`hybrid_rotation` 或 `hybrid_aggregate_first_rotation`。
2. 请求路径是否被识别成预期协议。
3. 聚合 API 的 `providerType` 是否和协议匹配。
4. 聚合 API 的 `status` 是否为 `active`。
5. 平台 Key 是否绑定了错误的 `aggregateApiId`。

### 只有某个模型不可用

检查：

1. 平台模型是否存在于模型目录。
2. 平台 Key 是否允许使用该模型。
3. 是否存在 enabled 的 V2 route。
4. route 的 `sourceKind` 是否为 `aggregate_api`。
5. route 的 `sourceId` 是否对应当前 active 聚合 API。
6. route 的 `upstreamModel` 是否填写为供应商真实模型名。
7. route 的 `priority` / `weight` 是否让预期线路处于当前优先级；`sortOrder` 和连接 `sort` 不会改变请求顺序。

典型错误：

```text
model_unavailable: gpt-5.5
```

### 上游鉴权失败

检查：

1. `authType` 是否正确。
2. 默认 Bearer 是否符合供应商要求。
3. 如果供应商要 `x-api-key` 或 `api-key`，是否启用了 `authCustomEnabled`。
4. `headerValueFormat` 是否应该是 `raw`。
5. userpass 模式是否同时配置了 username 和 password。
6. 自定义 header 名是否合法。

### 上游路径不对

检查：

1. `url` 是否已经包含 `/v1`。
2. 是否误把完整 URL 写进 `action`。
3. 是否启用了 action 导致原始路径被忽略。
4. 供应商是否需要 base URL 路径前缀，例如 `/api/anthropic`。

### 余额刷新失败

检查：

1. `balanceQueryEnabled` 是否为 true。
2. 模板是否选对：`generic`、`new_api`、`custom`。
3. `balanceQueryBaseUrl` 是否需要单独配置。
4. New API 是否需要单独 `balanceQueryAccessToken`。
5. custom 模板的 `remainingPath` 是否能取到数字。
6. custom 模板的 `multiplier` 是否按供应商单位换算。

### 请求日志显示 502，但上游实际是 403/429

聚合 API 对非 2xx 上游响应会归一为网关失败，最终可能以 502 返回给客户端；上游原始状态和错误体摘要会记录在请求日志错误字段中。排查时以请求日志 tooltip/详情中的 upstream 摘要为准。

### 聚合 API 被自动停用或没有触发自动停用

检查：

1. 该连接的 `autoToggleEnabled` 是否开启；旧记录和新建连接默认关闭。
2. 平台 Key 是否使用三种包含聚合来源的轮转之一；纯账号池不会修改聚合计数。
3. 上游错误是否包含明确的 daily/weekly/monthly quota 信号；JOJO 当前可能返回
   `reason=MONTHLY_LIMIT_EXCEEDED`。不要只看最终 429/502。
4. `consecutiveFailures` 是否来自三个独立业务请求；同一请求内部重试只计一次。
5. 中间是否出现成功请求；成功会立即把连续次数清零。
6. `status` 是否被人工设为 disabled；人工关闭不会被“解除熔断”或次日恢复改为 active。
7. 如果已经充值，使用“解除熔断”清当前自动状态；历史时间和原因继续保留是正常行为。

## 推荐配置规范

1. 每个供应商都填写清晰的 `supplierName`，例如 `New API - 主线路`。
2. 用模型 route 的 `priority` 表达主备层级，数值越大越先尝试；同层流量比例使用 `weight`。
3. 默认不要启用 action；只有供应商路径和客户端路径不一致时再启用。
4. 所有上游模型名都在模型目录 V2 route 中维护，不在连接记录上配置全局覆盖。
5. New API 优先用 `balanceQueryTemplate = new_api`。
6. Claude-compatible 供应商优先显式配置 `x-api-key` raw header。
7. Codex、Claude、Gemini 和 Responses 都由 V2 route 决定真实上游模型；远端发现只作为输入辅助。
8. `sortOrder` 和聚合连接 `sort` 只用于页面整理；不要把它们当作运行时优先级。
9. `CODEXMANAGER_ROUTE_STRATEGY=balanced` 只影响账号池 route 内的具体账号，不会均衡聚合 API route。
10. 只对确实按日结算额度的连接开启 `autoToggleEnabled`；普通 429 和临时网络错误不会触发熔断。
11. 需要长期停用时使用人工 `status = disabled`；不要依赖自动熔断代替人工管理状态。
