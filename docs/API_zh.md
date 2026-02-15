# Rscholar API 文档（中文）

本文档描述当前 `src/server/` 实现的 HTTP API。

## 基础地址

默认本地地址：

- `http://localhost:3000`

## 认证模型

- 公共接口不需要 API key。
- 管理接口需要 `X-API-Key` 且必须是管理员 key。

## 响应约定

- 公共任务接口：直接返回 JSON 对象。
- 管理接口：统一包裹格式
  - 成功：`{ "success": true, "data": ... }`
  - 失败：`{ "success": false, "error": { "code": "...", "message": "..." } }`

## 公共接口

## `GET /health`

健康检查。

### 响应 `200`

```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_secs": 123
}
```

## `POST /tasks`

提交异步检索任务。

### 请求字段

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `keyword` | string | 是 | 搜索关键词 |
| `ylo` | integer | 否 | 年份下限 |
| `enable_crossref` | boolean | 否 | 是否启用 Crossref 增强 |
| `sciif` | number | 否 | IF 最小阈值 |
| `jci` | number | 否 | JCI 最小阈值 |
| `sci` | string | 否 | SCI 分区匹配字符串 |
| `llm_strict_filter` | boolean | 否 | `true` 时 LLM 过滤为空不回退 |
| `content_help` | string | 否 | 研究意图描述（触发 LLM 扩展/相关性） |
| `source_include` | string[] | 否 | source 白名单 |
| `source_exclude` | string[] | 否 | source 黑名单 |

支持的 source 值：

- `openalex`
- `semanticscholar`
- `arxiv`
- `pubmed`
- `biorxiv`
- `medrxiv`

### 重要运行规则

- `source_include` 里有未知值会直接返回校验错误。
- 若设置了 `sciif/jci/sci` 但服务端未配置 `easyscholar.keys`，请求会被拒绝。
- 若关键词非英文，pipeline 会先尝试翻译成英文再检索。
- 关键词扩展基于翻译后的英文关键词。
- LLM 相关性过滤仅在 `content_help` 非空且 provider 可用时执行。

### 请求示例

```json
{
  "keyword": "机器学习 岩石 强度 预测",
  "ylo": 2021,
  "enable_crossref": true,
  "sciif": 3.0,
  "llm_strict_filter": false,
  "content_help": "关注机器学习和人工智能的应用",
  "source_include": ["openalex", "arxiv", "pubmed"],
  "source_exclude": ["semanticscholar"]
}
```

### 响应 `200`

```json
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "pending",
  "eta_seconds": 120
}
```

### 错误示例 `400`

```json
{
  "error": "Configuration error",
  "details": "Unsupported source(s): foo. Supported sources: openalex, semanticscholar, arxiv, pubmed, biorxiv, medrxiv"
}
```

## `GET /tasks/{id}`

查询任务状态与结果。

### 响应 `200`（运行中）

```json
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "running",
  "progress": {
    "step": "Searching papers",
    "percent": 10
  },
  "result": null,
  "error": null
}
```

### 响应 `200`（完成）

```json
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "progress": {
    "step": "Completed",
    "percent": 100
  },
  "result": {
    "total_papers": 101,
    "filtered_papers": 77,
    "csv_path": "output/20260210_183000_machine_learning/results.csv",
    "data": [
      {
        "title": "...",
        "authors": "...",
        "year": "2024",
        "venue": "...",
        "doi": "...",
        "url": "...",
        "pdf_url": "...",
        "snippet": "...",
        "abstract_text": "...",
        "if_score": "...",
        "jci_score": "...",
        "sci_partition": "..."
      }
    ]
  },
  "error": null
}
```

### 响应 `404`

```json
{
  "error": "Task not found",
  "details": "Task ID: ..."
}
```

## `GET /tasks/{id}/download`

下载任务 CSV。

### 成功

- 状态码：`200`
- `Content-Type: text/csv; charset=utf-8`

### 常见错误

- `400`：任务未完成
- `404`：结果无 CSV 路径
- `500`：文件读取或响应构建失败

## `GET /tasks/{id}/bibtex`

根据任务结果生成并下载 BibTeX。

### 成功

- 状态码：`200`
- `Content-Type: application/x-bibtex; charset=utf-8`

### 常见错误

- `400`：任务未完成
- `404`：无论文结果
- `500`：解析或响应构建失败

## 管理接口（Admin）

所有管理接口前缀：

- `/api/v1/admin`

所有管理接口都需要：

- 请求头：`X-API-Key: <admin-key>`

中间件行为：

- 缺失/非法 key -> `401`
- 合法但非 admin key -> `403`

## Key 管理

## `GET /api/v1/admin/keys?page=1&limit=20`

分页列出 API keys。

## `POST /api/v1/admin/keys`

创建 API key。

### 请求

```json
{
  "name": "analytics-bot",
  "is_admin": false,
  "rate_limit_rps": 10
}
```

## `GET /api/v1/admin/keys/{id}`

按 id 查询 key。

## `PATCH /api/v1/admin/keys/{id}`

更新 key 名称和/或限速。

### 请求

```json
{
  "name": "new-name",
  "rate_limit_rps": 20
}
```

## `DELETE /api/v1/admin/keys/{id}`

删除 key。

## 缓存管理

## `GET /api/v1/admin/cache/journals?page=1&limit=20`

分页列出期刊缓存。

## `GET /api/v1/admin/cache/stats`

缓存统计。

字段示例：

- `total_entries`
- `oldest_entry_at`
- `newest_entry_at`
- `total_lookups`
- `cache_hits`
- `hit_rate`

## `DELETE /api/v1/admin/cache/journals`

清空期刊缓存。

## `DELETE /api/v1/admin/cache/journals/{name}`

删除指定期刊缓存。

## 统计分析

## `GET /api/v1/admin/stats/overview`

总体统计（含缓存命中率）。

字段包括：

- `total_searches`
- `total_papers_returned`
- `unique_keywords`
- `unique_journals`
- `avg_papers_per_search`
- `cache_hit_rate`

## `GET /api/v1/admin/stats/keywords?limit=20`

高频关键词统计。

## `GET /api/v1/admin/stats/journals?limit=20`

高频期刊统计。

## `GET /api/v1/admin/stats/daily?days=7`

按天统计检索次数与论文数。

## 系统状态

## `GET /api/v1/admin/system`

系统概览。

字段包括：

- `version`
- `uptime_secs`
- `db_size_bytes`
- `active_tasks`
- `total_api_keys`
- `cache_entries`
- `total_searches`

## 错误码

管理接口包裹错误码：

- `UNAUTHORIZED`
- `FORBIDDEN`
- `BAD_REQUEST`
- `NOT_FOUND`
- `INTERNAL_ERROR`
- `RATE_LIMITED`

公共任务接口错误使用 `{error, details}` 结构。

## CSV 输出字段

当前 `results.csv` 字段：

- `title`
- `authors`
- `year`
- `venue`
- `doi`
- `url`
- `pdf_url`
- `snippet`
- `abstract_text`
- `if_score`
- `jci_score`
- `sci_partition`

## 客户端注意事项

- 任务状态和结果持久化在 DB，可在服务重启后继续查询。
- 完成/失败任务会从内存缓存清理，但 DB 仍可查询。
- `content_help` 是触发 LLM 扩展与相关性筛选的关键输入。
- 预印本（`arXiv`/`bioRxiv`/`medRxiv`）不会被 IF/JCI/SCI 过滤剔除。
