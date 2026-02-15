# Rscholar

Rscholar 是一个基于 Rust 的学术文献检索服务，支持异步任务执行、多源检索、元数据增强、期刊指标过滤，以及 CSV/BibTeX 导出。

本文档已按当前代码实现更新。

## 功能总览

- 多源检索：
  - `openalex`
  - `semanticscholar`
  - `arxiv`
  - `pubmed`
  - `biorxiv`
  - `medrxiv`
- 异步任务链路：`POST /tasks` -> 轮询 -> 下载
- 可选增强：
  - Crossref（仅补全缺 DOI 论文）
  - Semantic Scholar DOI 批量补全摘要/PDF
- EasyScholar 期刊指标（带 SQLite 缓存与调度）
- 指标筛选：`sciif`、`jci`、`sci`
  - 预印本不会因为指标过滤被剔除
- `content_help` 存在时可启用 LLM 相关阶段
- 导出：
  - 任务 JSON 结果
  - `results.csv`
  - BibTeX（`/tasks/{id}/bibtex`）

## 当前实现中的关键行为

- 内置关键词翻译阶段：
  - 输入关键词若为非英文，先尝试翻译为英文再检索
  - 后续搜索和关键词扩展都使用翻译后的英文词
  - 翻译失败自动回退原关键词，不中断任务
- 扩展词基于英文关键词生成
- source 严格校验：
  - `source_include` 中有未知 source 会直接报错
- 请求级 `enable_llm_filter` 已移除
  - 是否执行 LLM 相关性过滤由 pipeline 决定（如 `content_help`）
- 输出目录命名：
  - `output/{timestamp}_{keyword}`

## 运行模式

- HTTP 服务模式（主模式）：
  - `Rscholar server --port 3000 --serve-static front/dist`
- CLI 搜索模式（辅助）：
  - `Rscholar search ...`

## 快速启动

Linux/macOS：

```bash
./start.sh
```

Windows PowerShell：

```powershell
.\start.ps1
```

脚本会依次执行：

1. 安装前端依赖（`npm ci`/`npm install`）
2. 构建前端（`npm run build`）
3. 构建后端 release（`cargo build --release`）
4. 启动后端并托管前端静态资源

默认地址：`http://localhost:3000`

## API 概览

公开接口：

- `GET /health`
- `POST /tasks`
- `GET /tasks/{id}`
- `GET /tasks/{id}/download`
- `GET /tasks/{id}/bibtex`

管理接口：

- 前缀：`/api/v1/admin/*`
- 认证：`X-API-Key`（必须是管理员 key）
- 功能：key 管理、缓存管理、统计、系统状态

完整接口说明见 `docs/API.md` 与 `docs/API_zh.md`。

## `POST /tasks` 请求字段

当前支持字段：

- `keyword`（必填）
- `ylo`
- `enable_crossref`
- `sciif`
- `jci`
- `sci`
- `llm_strict_filter`
- `content_help`
- `source_include`
- `source_exclude`

注意：

- 开启指标筛选需服务端配置 `easyscholar.keys`
- `source_include`/`source_exclude` 大小写不敏感
- source 可选值：
  - `openalex`、`semanticscholar`、`arxiv`、`pubmed`、`biorxiv`、`medrxiv`

## Pipeline 阶段

1. 关键词翻译（LLM，必要时）
2. 关键词扩展（LLM，基于英文关键词）
3. 多源并发检索
4. 合并与去重
5. Crossref 增强（仅缺 DOI）
6. Semantic Scholar DOI 批量增强
7. 期刊指标查询与写回
8. 指标筛选（`sciif/jci/sci`，预印本豁免）
9. LLM 相关性筛选（`content_help` 非空时）
10. 回退策略（严格/非严格）
11. 保存 CSV、记录统计、完成任务

## 项目结构

```text
src/
  cli/                     CLI 命令（search / server / init-admin）
  db/                      SQLite schema + CRUD（tasks/keys/cache/analytics）
  llm/                     LLM provider、关键词扩展、关键词翻译
  ranking/                 EasyScholar key pool + ranking service scheduler
  server/                  HTTP API（routes/handlers/pipeline/middleware/admin）
  sources/                 openalex/semanticscholar/arxiv/pubmed/xrxiv/crossref
  error.rs                 统一错误类型
  traffic.rs               流量统计
  unified.rs               统一输出结构（CLI 路径）
front/                     Vite 前端
docs/                      API 与架构文档
tests/                     集成与在线测试
```

## 配置说明

主配置文件：`config.toml`

主要章节：

- `[server]` 地址与端口
- `[easyscholar]` ranking API keys
- `[ranking]` 调度/租约/key 健康参数
- `[llm]` provider 配置与优先级
- `[search]` 默认 ylo、source 限制、默认 source
- `[search.arxiv]`、`[search.pubmed]`、`[search.xrxiv]` 子配置

## 管理员 Key 初始化

首次初始化管理员 key：

```bash
cargo run -- init-admin --name Admin
```

管理接口调用时加请求头：

```text
X-API-Key: <your-admin-key>
```

## 输出与持久化

- 任务状态与结果持久化到 SQLite（`data/rscholar.db`）
- CSV 路径：
  - `output/{timestamp}_{sanitized_keyword}/results.csv`
- 任务内存缓存定时清理（完成/失败任务 TTL）
- 服务重启后：
  - 任务仍可从 DB 查询
  - 中断中的运行任务会被恢复为失败状态

## 安全建议

- 管理接口通过 `X-API-Key` 认证
- API key 在 DB 中为 HMAC-SHA256 哈希存储
- 生产环境不要把真实 key 提交到版本库
- 建议使用环境变量或密钥管理服务

## 开发说明

- 在受限环境中，前端构建可能需要提升权限
- 集成/在线测试在 Windows 环境可能较慢

## 相关文档

- `docs/API.md`
- `docs/API_zh.md`
- `docs/ARCHITECTURE.md`
