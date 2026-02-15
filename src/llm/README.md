# LLM 扩展指南（统一链路版）

这份文档说明现在的统一链路：

`server -> LlmRelevanceFilter::build_from_config(...) -> provider 调度`

你不需要再在 `main.rs` 或 `cli/server.rs` 里手工加 provider 分支。

---

## 1) 代码模块说明

- `src/llm/mod.rs`
  - 定义 `LlmProvider` trial 模板（trait）
  - 定义 `LlmRelevanceFilter`（并发、fallback、批量调度）
  - 提供 `build_from_config`：从 TOML 自动发现并初始化 provider
  - 通过自动注册表加载 `src/llm/providers/*.rs`
- `src/llm/provider_core.rs`
  - 统一 `reqwest` 请求执行 trial（超时、鉴权头、错误处理）
  - 只负责请求与基础日志，不内置具体响应后处理
- `src/llm/providers/provider_template.rs`
  - 新 provider 模板（复制后最小改动即可接入）
- `src/llm/providers/aiping.rs`
  - AIPing provider（SSE 解析）
- `src/llm/providers/siliconflow.rs`
  - SiliconFlow provider（JSON）
- `src/llm/providers/bigmodel.rs`
  - BigModel provider（GLM，JSON）
- `src/llm/keyword_expansion.rs`
  - 关键词扩展（复用同一 provider 抽象）

---

## 2) Trial 模板（统一接口）

新增 provider 时，实现下面 trait 即可：

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat_completion(&self, messages: Vec<ChatMessage>) -> Result<String>;
}
```

约定：

- 输入：`Vec<ChatMessage>`
- 输出：`Result<String>`（最终文本）
- 错误：统一走 `GscholarError`

---

## 3) 配置注册（TOML）

现在使用两层配置：

- `llm.providers`：provider 执行顺序（含 fallback 顺序）
- `llm.registry.<name>`：provider 的配置注册表

示例：

```toml
[llm]
default_provider = "bigmodel"
enable_filter = true
strict_filter = false
providers = ["bigmodel", "siliconflow"]

[llm.registry.bigmodel]
api_key = "xxx"
model = "GLM-4.7-Flash"
endpoint = "https://open.bigmodel.cn/api/paas/v4/chat/completions"
```

> `build_from_config` 会自动按顺序加载、记录日志、失败自动跳过并继续 fallback。

---

## 4) 手把手：BigModel 作为案例

BigModel 已经内置实现（`src/llm/providers/bigmodel.rs`），你只需要写配置，不需要改业务代码：

### 步骤 1：配置 `config.toml`

```toml
[llm]
default_provider = "bigmodel"
enable_filter = true
strict_filter = false
providers = ["bigmodel", "siliconflow"]

[llm.registry.bigmodel]
api_key = "xxx"
model = "GLM-4.7-Flash"
endpoint = "https://open.bigmodel.cn/api/paas/v4/chat/completions"
```

### 步骤 2：启动服务

```bash
cargo run -- server
```

### 步骤 3：看日志确认初始化

你会看到类似日志：

- `Building LLM providers from config`
- `LLM provider initialized ... provider=BigModel`
- `LLM relevance filter enabled ... primary=BigModel`

---

## 5) 新增“全新协议” Provider 的最小改动

如果是 JSON Chat API 兼容提供商，通常只要改 TOML（改 endpoint/model/key）即可。

如果是全新协议（非兼容），建议：

1. 复制 `src/llm/providers/provider_template.rs` 为 `src/llm/providers/xxx.rs`
2. 改 endpoint/model/payload/parser，并保留 `register(...)`
3. 在 TOML 中加 `[llm.registry.xxx]`（可选加入 `llm.providers` 顺序）

> 不需要再改 `src/llm/mod.rs`。`build.rs` 会自动扫描并注册 `src/llm/providers/*.rs`。  
> `parse_response_body` 必须在 provider 内自行实现（可按流式/非流式/思考过滤自由处理）。

---

## 6) 可用性保障（日志与测试）

已内置：

- 初始化日志（加载顺序、模型、endpoint、失败原因）
- provider 构建失败自动跳过
- 单元测试覆盖：
  - `LlmRelevanceFilter::build_from_config`（开启/关闭、provider 配置）
  - `server::config::LlmSection` 的 provider 顺序和注册解析
