# Rscholar 架构文档（中文）

本文档描述当前代码实现的系统架构、模块边界、任务执行流程与运行时数据流。

---

## 1. 系统定位

Rscholar 是一个面向学术检索场景的异步任务型服务，核心能力是：

- 多源检索（OpenAlex / Semantic Scholar / arXiv / PubMed / bioRxiv / medRxiv）
- 元数据增强（Crossref + Semantic Scholar DOI 批量回填）
- 期刊指标查询与筛选（EasyScholar + 本地缓存）
- LLM 辅助（关键词翻译、关键词扩展、相关性过滤）
- 结果导出（CSV / BibTeX）
- SQLite 持久化（任务、缓存、统计）

---

## 2. 总体架构（ASCII）

```text
+-------------------- Client ---------------------+
|  Web Frontend / Script / CLI / External Caller  |
+--------------------------+----------------------+
                           |
                           | HTTP
                           v
+--------------------- Axum API Server ----------------------+
|  Public Routes: /health /tasks /tasks/{id} /download /bibtex|
|  Admin Routes : /api/v1/admin/* (可开关 + X-API-Key 鉴权)    |
+--------------------------+----------------------------------+
                           |
                           v
+----------------------- AppState ----------------------------+
| config | task_store(DashMap) | db_pool(SQLite WAL)         |
| llm_filter(optional) | ranking_service(optional)            |
+--------------------------+----------------------------------+
                           |
                           v
+------------------- Pipeline Orchestrator -------------------+
| 0. Keyword Translation (LLM, when needed)                  |
| 0.5 Keyword Expansion (LLM, based on English keyword)      |
| 1. Parallel Search (multi-source)                          |
| 2. Merge + Dedup                                           |
| 3. Crossref Enrichment (missing DOI only)                  |
| 4. SemanticScholar DOI Enrichment                          |
| 5. Ranking Lookup + Ranking Filters                        |
| 6. LLM Relevance Filter (if content_help provided)         |
| 7. Fallback + CSV Export + Analytics                       |
+--------------------------+----------------------------------+
                           |
                           v
+----------------------- SQLite ------------------------------+
| tasks | api_keys | journal_cache | search_logs | cache_stats|
+-------------------------------------------------------------+
```

---

## 3. 代码模块结构

```text
src/
  cli/
    mod.rs
    server.rs           # server / init-admin 命令
    search.rs           # CLI 检索流程（辅助）

  server/
    mod.rs
    routes.rs           # 路由组装 + admin 挂载
    handlers.rs         # 公共任务接口
    admin.rs            # 管理接口实现
    middleware.rs       # require_admin
    state.rs            # AppState
    config.rs           # 配置结构与加载
    task.rs             # 内存任务模型与缓存
    recovery.rs         # 启动恢复中断任务
    pipeline.rs         # 主 pipeline
    pipeline/
      search_stage.rs
      merge.rs
      keyword_translation.rs
      keyword_expansion.rs
      llm_filter.rs
      fallback.rs
      export.rs
      analytics.rs

  sources/
    openalex.rs
    semanticscholar.rs
    arxiv.rs
    pubmed.rs
    xrxiv.rs
    crossref.rs

  ranking/
    client.rs / pool.rs / service.rs / types.rs

  llm/
    mod.rs
    keyword_translation.rs
    keyword_expansion.rs
    providers/*

  db/
    schema.rs
    tasks.rs
    api_keys.rs
    journal_cache.rs
    analytics.rs
```

---

## 4. 运行时组件职责

## 4.1 API 层

- 入口：`src/server/routes.rs`
- 公共接口：
  - `GET /health`
  - `POST /tasks`
  - `GET /tasks/{id}`
  - `GET /tasks/{id}/download`
  - `GET /tasks/{id}/bibtex`
- 管理接口：`/api/v1/admin/*`（由配置开关决定是否挂载）

## 4.2 鉴权层（Admin）

- 文件：`src/server/middleware.rs`
- 逻辑：
  - 检查 `X-API-Key`
  - 在 `api_keys` 表里校验 key
  - 要求 `is_admin = true`
- 失败返回：`401` 或 `403`

## 4.3 任务层

- 内存缓存：`TaskStore`（DashMap）
- 持久化：`db/tasks.rs`
- 策略：
  - 创建任务走 DB-first
  - 查询任务走 memory-first + DB fallback
  - 完成/失败任务内存 TTL 清理

## 4.4 数据源层

- OpenAlex：检索主来源之一
- Semantic Scholar：检索来源 + DOI 批量增强
- arXiv：Atom API，带双端点回退
- PubMed：E-utilities (esearch + efetch)
- xrxiv：bioRxiv / medRxiv，分页并发抓取
- Crossref：为缺 DOI 论文补齐 metadata

## 4.5 Ranking 层

- `RankingService` + key pool
- 处理 venue 批量查询
- journal_cache 缓存命中优先
- 把 IF/JCI/SCI 写回论文项

## 4.6 LLM 层

- 关键词翻译：非英文关键词 -> 英文关键词
- 关键词扩展：基于英文关键词扩展检索词
- 相关性过滤：按 `content_help` 判断论文是否保留

---

## 5. 任务执行工作流（ASCII 时序）

```text
Client                  API                Pipeline                 DB
  | POST /tasks          |                      |                     |
  |--------------------->| validate request     |                     |
  |                      | build config         |                     |
  |                      | insert task -------->| INSERT tasks        |
  |                      |<---------------------|                     |
  |<---------------------| task_id              |                     |
  |                      | spawn async -------->| start               |
  |                      |                      | progress update     |
  |                      |                      |-------> update DB   |
  | GET /tasks/{id}      |                      |                     |
  |--------------------->| memory first / db fallback                |
  |<---------------------| running/completed    |                     |
```

---

## 6. Pipeline 细化流程（ASCII）

```text
[Input keyword]
      |
      v
+------------------------------+
| Stage 0: Keyword Translation |
| - 非英文时尝试翻译为英文       |
| - 失败回退原词                |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 0.5: Keyword Expansion |
| - 基于英文关键词扩展           |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 1: Parallel Search     |
| openalex / ss / arxiv / ...  |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 2: Merge + Dedup       |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 3: Crossref Enrich     |
| (missing DOI only)           |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 4: SS DOI Enrich       |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 5: Ranking Lookup      |
| + IF/JCI/SCI filter          |
| (preprint venues exempt)     |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 6: LLM Relevance       |
| (content_help 非空才执行)     |
+--------------+---------------+
               |
               v
+------------------------------+
| Stage 7: Fallback + Export   |
| save results.csv + analytics |
+------------------------------+
```

---

## 7. source 选择与校验流程

```text
request.source_include ?
    | yes                          no
    v                              v
 base = source_include       base = config.search.enabled_sources
    |                              |
    +------------> apply source_exclude
                          |
                          v
                 normalize + de-dup
                          |
                          v
                validate against allowed set
                          |
              invalid -> Validation error
```

允许 source：

- `openalex`
- `semanticscholar`
- `arxiv`
- `pubmed`
- `biorxiv`
- `medrxiv`

---

## 8. 筛选与回退策略

## 8.1 Ranking 筛选

- 当设置 `sciif/jci/sci` 时触发
- 对期刊论文按指标筛选
- 对预印本 venue（arXiv/bioRxiv/medRxiv）不按指标剔除

## 8.2 LLM 相关性筛选

- 触发条件：`content_help` 非空且 LLM 可用
- 若过滤后为空：
  - `llm_strict_filter = true`：保持空
  - `llm_strict_filter = false`：回退到未过滤结果

---

## 9. 数据持久化与恢复

## 9.1 SQLite 模式

- WAL
- busy_timeout
- 同步策略 NORMAL

## 9.2 表

- `tasks`：任务状态/进度/结果
- `api_keys`：管理 key（哈希存储）
- `journal_cache`：期刊指标缓存
- `search_logs` + `journal_hits`：检索统计
- `cache_stats`：缓存命中统计

## 9.3 服务重启恢复

- 启动时扫描运行中任务
- 将中断任务标记为失败
- 任务查询仍可通过 DB fallback 获取历史结果

---

## 10. 管理接口实现说明（你关心的点）

管理员功能并不是直接读系统文件，而是主要基于 DB 和运行时状态：

- key 管理：`db/api_keys.rs`
- 缓存管理：`db/journal_cache.rs`
- 统计：`db/analytics.rs`
- 系统状态：任务数 + DB 文件大小 + DB 统计

对应实现文件：

- 路由挂载：`src/server/routes.rs`
- 鉴权：`src/server/middleware.rs`
- 业务处理：`src/server/admin.rs`

---

## 11. 安全边界与建议

- Admin 依赖 `X-API-Key`，请确保 key 不泄露
- 建议把管理路由默认关闭，仅在内网/跳板机开启
- 公网部署建议配合反向代理/WAF 做：
  - IP 白名单
  - 访问频率限制
  - TLS 强制
- 配置中的真实密钥建议迁移到环境变量或秘密管理服务

---

## 12. 可演进方向

- 为下载接口增加签名 token（避免仅凭 task_id 下载）
- 为 admin 增加来源 IP 白名单（应用层）
- 为 pipeline 增加可观测性追踪（trace_id/span）
- 为 source 错误分类（网络/限流/解析）提供统一错误码
