/**
 * API Documentation Page - Static page showing API usage
 */

import { router } from '../main.js';

export class ApiPage {
  render() {
    return `
      <!-- Header -->
      <header class="header">
        <div class="container header-container">
          <button id="history-toggle" class="btn-history" aria-label="查询历史">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="10"></circle>
              <polyline points="12 6 12 12 16 14"></polyline>
            </svg>
            <span>历史记录</span>
          </button>
          <a href="/" class="header-logo">RustScholar</a>
          <span class="header-link header-link-active">API 文档</span>
        </div>
      </header>

      <!-- API Documentation -->
      <section class="api-section">
        <div class="container">
          <div class="api-card">
            <h1 class="api-title">API 接口文档</h1>
            <p class="api-intro">RustScholar 提供 RESTful API 接口，支持程序化调用。以下是各接口的使用方法。</p>
            
            <!-- Base URL -->
            <div class="api-block">
              <h2 class="api-heading">基础 URL</h2>
              <div class="code-block">
                <code>http://c76d.abrdns.com</code>
              </div>
            </div>

            <!-- Health Check -->
            <div class="api-block">
              <h2 class="api-heading">1. 健康检查</h2>
              <p class="api-desc">检查服务是否正常运行</p>
              
              <div class="api-endpoint">
                <span class="method method-get">GET</span>
                <code>/health</code>
              </div>
              
              <h3 class="api-subheading">cURL 示例</h3>
              <div class="code-block">
                <pre>curl http://c76d.abrdns.com/health</pre>
              </div>
              
              <h3 class="api-subheading">响应示例</h3>
              <div class="code-block">
                <pre>{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_secs": 3600
}</pre>
              </div>
            </div>

            <!-- Create Task -->
            <div class="api-block">
              <h2 class="api-heading">2. 创建搜索任务</h2>
              <p class="api-desc">提交一个新的后台搜索任务</p>
              
              <div class="api-endpoint">
                <span class="method method-post">POST</span>
                <code>/tasks</code>
              </div>
              
              <h3 class="api-subheading">请求参数</h3>
              <table class="api-table">
                <thead>
                  <tr>
                    <th>字段</th>
                    <th>类型</th>
                    <th>必填</th>
                    <th>说明</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td><code>keyword</code></td>
                    <td>String</td>
                    <td>✓</td>
                    <td>搜索关键词</td>
                  </tr>
                  <tr>
                    <td><code>ylo</code></td>
                    <td>Integer</td>
                    <td></td>
                    <td>起始年份 (如 2023)</td>
                  </tr>
                  <tr>
                    <td><code>sciif</code></td>
                    <td>Float</td>
                    <td></td>
                    <td>最低影响因子</td>
                  </tr>
                  <tr>
                    <td><code>jci</code></td>
                    <td>Float</td>
                    <td></td>
                    <td>最低 JCI 分数</td>
                  </tr>
                  <tr>
                    <td><code>sci</code></td>
                    <td>String</td>
                    <td></td>
                    <td>SCI 分区 (Q1/Q2/Q3/Q4)</td>
                  </tr>
                  <tr>
                    <td><code>content_help</code></td>
                    <td>String</td>
                    <td></td>
                    <td>研究方向描述</td>
                  </tr>
                  <tr>
                    <td><code>llm_strict_filter</code></td>
                    <td>Boolean</td>
                    <td></td>
                    <td>LLM 严格模式（true=过滤空即空结果，false=回退未过滤）</td>
                  </tr>
                  <tr>
                    <td><code>source_include</code></td>
                    <td>String[]</td>
                    <td></td>
                    <td>本次任务包含的检索源（通过 GET /sources 获取可用列表）</td>
                  </tr>
                  <tr>
                    <td><code>source_exclude</code></td>
                    <td>String[]</td>
                    <td></td>
                    <td>本次任务排除的检索源</td>
                  </tr>
                </tbody>
              </table>
              <p class="api-note">
                说明：若传入 <code>source_include</code>，本次任务将按该列表检索；再应用 <code>source_exclude</code> 排除来源。若两者都不传，则使用服务端 <code>search.enabled_sources</code> 默认值。
              </p>
              
              <h3 class="api-subheading">cURL 示例</h3>
              <div class="code-block">
                <pre>curl -X POST http://c76d.abrdns.com/tasks \\
  -H "Content-Type: application/json" \\
  -d '{
    "keyword": "ai",
    "ylo": 2023,
    "sciif": 5.0,
    "llm_strict_filter": false,
    "source_include": ["openalex", "arxiv"],
    "source_exclude": ["semanticscholar"],
    "content_help": "关注岩石力学中的机器学习预测方法"
  }'</pre>
              </div>
              
              <h3 class="api-subheading">Python 示例</h3>
              <div class="code-block">
                <pre>import requests

response = requests.post(
    "http://c76d.abrdns.com/tasks",
    json={
        "keyword": "ai",
        "ylo": 2023,
        "sciif": 5.0,
        "llm_strict_filter": False,
        "source_include": ["openalex", "arxiv"],
        "source_exclude": ["semanticscholar"],
        "content_help": "关注岩石力学中的机器学习预测方法"
    }
)

task_id = response.json()["task_id"]
print(f"Task created: {task_id}")</pre>
              </div>
              
              <h3 class="api-subheading">响应示例</h3>
              <div class="code-block">
                <pre>{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "pending",
  "eta_seconds": 120
}</pre>
              </div>
            </div>

            <!-- Get Task Status -->
            <div class="api-block">
              <h2 class="api-heading">3. 获取任务状态</h2>
              <p class="api-desc">轮询任务的执行进度</p>
              
              <div class="api-endpoint">
                <span class="method method-get">GET</span>
                <code>/tasks/{id}</code>
              </div>
              
              <h3 class="api-subheading">cURL 示例</h3>
              <div class="code-block">
                <pre>curl http://c76d.abrdns.com/tasks/550e8400-e29b-41d4-a716-446655440000</pre>
              </div>
              
              <h3 class="api-subheading">响应示例 (运行中)</h3>
              <div class="code-block">
                <pre>{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "running",
  "progress": {
    "step": "Fetching papers...",
    "percent": 25
  }
}</pre>
              </div>
              
              <h3 class="api-subheading">响应示例 (已完成)</h3>
              <div class="code-block">
                <pre>{
  "task_id": "...",
  "status": "completed",
  "progress": { "step": "Done", "percent": 100 },
  "result": {
    "total_papers": 45,
    "filtered_papers": 30,
    "csv_path": "output/keyword_20260208_120000/results.csv",
    "data": [...]
  }
}</pre>
              </div>
            </div>

            <!-- Download CSV -->
            <div class="api-block">
              <h2 class="api-heading">4. 下载 CSV 结果</h2>
              <p class="api-desc">下载任务完成后的 CSV 文件</p>
              
              <div class="api-endpoint">
                <span class="method method-get">GET</span>
                <code>/tasks/{id}/download</code>
              </div>
              
              <h3 class="api-subheading">cURL 示例</h3>
              <div class="code-block">
                <pre>curl -O http://c76d.abrdns.com/tasks/{task_id}/download</pre>
              </div>
            </div>

            <!-- Download BibTeX -->
            <div class="api-block">
              <h2 class="api-heading">5. 下载 BibTeX</h2>
              <p class="api-desc">下载 BibTeX 格式引用文件</p>
              
              <div class="api-endpoint">
                <span class="method method-get">GET</span>
                <code>/tasks/{id}/bibtex</code>
              </div>
              
              <h3 class="api-subheading">cURL 示例</h3>
              <div class="code-block">
                <pre>curl -O http://c76d.abrdns.com/tasks/{task_id}/bibtex</pre>
              </div>
            </div>

            <!-- Back Button -->
            <div class="api-footer">
              <a href="/" class="btn btn-primary">
                <svg class="btn-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <line x1="19" y1="12" x2="5" y2="12"></line>
                  <polyline points="12 19 5 12 12 5"></polyline>
                </svg>
                <span>返回搜索</span>
              </a>
            </div>
          </div>
        </div>
      </section>
    `;
  }

  mount() {
    document.getElementById('history-toggle')?.addEventListener('click', () => window.toggleSidebar());

    // Handle internal links
    document.querySelectorAll('a[href^="/"]').forEach(link => {
      link.addEventListener('click', (e) => {
        e.preventDefault();
        router.navigate(link.getAttribute('href'));
      });
    });
  }
}
