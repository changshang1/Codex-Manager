"use client";

import type { MessageCatalog } from "../types";

export const EN_AGGREGATE_API_MESSAGES: MessageCatalog = {
  "Upstream routing": "Upstream routing",
  显式路由: "Explicit routes",
  "这里只管理上游连接；模型路由在“模型管理”中显式配置，页面不会访问供应商 `/models`。":
    "This page only manages upstream connections. Model routes are configured explicitly in Model Management, and the page never accesses a supplier `/models` endpoint.",
  "新建聚合 API": "New aggregate API",
  已有模型路由: "Has model routes",
  测试失败: "Tests failed",
  "测试中...": "Testing...",
  上游连接: "Upstream connections",
  "连通性测试只使用已配置路由对应的模型。":
    "Connection tests only use models referenced by configured routes.",
  "通用兼容（Codex + Claude）": "Compatible (Codex + Claude)",
  "按请求路径原样转发 Codex 与 Claude 协议；自定义 action 会自动关闭。":
    "Forwards Codex and Claude protocols using the incoming path; custom action is disabled automatically.",
  "例如：https://example.com/v1": "For example: https://example.com/v1",
  模型路由: "Model routes",
  供应商: "Supplier",
  连通性: "Connectivity",
  连通性测试成功: "Connection test succeeded",
  连通性测试失败: "Connection test failed",
  "聚合 API 已删除": "Aggregate API deleted",
  显示密钥: "Show key",
  隐藏密钥: "Hide key",
  复制密钥: "Copy key",
  "暂无聚合 API，点击右上角新建":
    "No aggregate APIs yet. Create one from the top-right corner.",
  "测试 route": "Test route",
  "删除连接时会同时删除引用它的模型路由。":
    "Deleting a connection also deletes model routes that reference it.",
  "配置一个最小转发上游，保存 URL 和密钥后即可用于平台密钥轮转。":
    "Configure a minimal forwarding upstream; after saving its URL and key it can be used for API-key rotation.",
  "New API 用户 ID": "New API user ID",
  "URL": "URL",
  "action path": "action path",
  "余额": "Balance",
  "余额 Access Token": "Balance access token",
  "余额倍率必须大于 0": "Balance multiplier must be greater than 0",
  "余额接口基础地址": "Balance API base URL",
  "余额刷新完成：{count} 个成功": "Balance refresh completed: {count} succeeded",
  "余额刷新完成：{success} 个成功，{fail} 个失败":
    "Balance refresh completed: {success} succeeded, {fail} failed",
  "余额已刷新": "Balance refreshed",
  "余额查询失败": "Balance query failed",
  "余额查询失败 {reason}": "Balance query failed: {reason}",
  "余额检测": "Balance check",
  "全部测试完成，{count} 个连通": "All tests completed, {count} connected",
  "刷新余额": "Refresh balance",
  "同步失败": "Sync failed",
  "套餐": "Plan",
  "已用": "Used",
  "开启后可在聚合 API 列表手动刷新并显示余额。":
    "When enabled, balances can be manually refreshed and displayed in the aggregate API list.",
  "总额": "Total",
  "批量刷新余额失败": "Batch balance refresh failed",
  "批量测试失败": "Batch test failed",
  "折算": "Conversion",
  "暂无可测试的聚合 API": "No aggregate APIs available to test",
  "暂无已启用余额检测的聚合 API":
    "No aggregate APIs with balance checking enabled",
  "未启用": "Disabled",
  "未查询": "Not queried",
  "查询失败": "Query failed",
  "查询模板": "Query template",
  "模板": "Template",
  "测试全部": "Test all",
  "测试完成：{success} 个连通，{fail} 个失败":
    "Test completed: {success} connected, {fail} failed",
  "留空则从 URL 推断服务根地址": "Leave blank to infer the service root from the URL",
  "留空则使用上方 URL": "Leave blank to use the URL above",
  "留空则使用密钥": "Leave blank to use the API key",
  "留空则保持原值或使用密钥":
    "Leave blank to keep the existing value or use the API key",
  "请输入余额字段路径": "Enter the balance field path",
  "请输入自定义余额查询路径": "Enter the custom balance query path",
  "通用余额": "Generic balance",
  人工已启用: "Manually enabled",
  人工启用: "Manual enablement",
  自动启停: "Automatic enable/disable",
  "按日额度异常连续达到阈值后自动停用，并在下一自然日恢复；不会改变人工启停状态。":
    "Automatically disables this connection after the daily-quota failure threshold is reached and restores it on the next calendar day; manual status is unchanged.",
  "保存关闭后将立即解除当前自动停用状态。":
    "Saving with this setting off immediately clears the current automatic suspension.",
  自动熔断: "Automatic circuit breaker",
  自动启停已关闭: "Automatic control off",
  已自动停用: "Automatically disabled",
  等待人工启用: "Waiting for manual enablement",
  监控中: "Monitoring",
  连续失败: "Consecutive failures",
  上次自动停用: "Last automatic disable",
  停用原因: "Disable reason",
  上游周期额度已耗尽: "Upstream periodic quota exhausted",
  "未知熔断原因：{reason}": "Unknown circuit-breaker reason: {reason}",
  解除熔断: "Clear circuit breaker",
  "解除中...": "Clearing...",
  自动熔断已解除: "Automatic circuit breaker cleared",
  解除熔断失败: "Failed to clear circuit breaker",
  上游协议: "Upstream protocol",
  "透传（不转换，直连上游）":
    "Passthrough (no conversion, connect directly to upstream)",
  "Chat Completions（通过 Chat Completions 桥接）":
    "Chat Completions (bridged through Chat Completions)",
  "选择 Chat Completions 可将 Responses 请求桥接到仅支持 Chat Completions 的上游。":
    "Choose Chat Completions to bridge Responses requests to upstreams that only support Chat Completions.",
};
