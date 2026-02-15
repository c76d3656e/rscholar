/**
 * Home Page - Search Form and Results
 */

import { createTask, pollTaskStatus, downloadCSV, downloadBibTeX, fetchSources } from '../api/client.js';
import { historyManager } from '../utils/history.js';
import { router, renderHistoryList, escapeHtml } from '../main.js';

export class HomePage {
  constructor() {
    this.currentTaskId = null;
    this.currentPapers = [];
    this.currentSortColumn = 'if_score';
    this.currentSortDirection = 'desc';
  }

  render() {
    const currentYear = new Date().getFullYear();
    const defaultYear = currentYear - 5;

    return `
      <!-- Header with History Toggle -->
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
          <a href="/docs" class="header-link">API 文档</a>
        </div>
      </header>

      <!-- Hero Section -->
      <section class="hero">
        <div class="hero-decoration">
          <div class="hero-circle hero-circle-1"></div>
          <div class="hero-circle hero-circle-2"></div>
          <div class="hero-square"></div>
        </div>
        <div class="container">
          <h1 class="hero-title">RustScholar</h1>
          <p class="hero-subtitle">学术文献智能搜索与筛选平台</p>
        </div>
      </section>

      <!-- Search Section -->
      <section class="search-section">
        <div class="container">
          <div class="search-card">
            <h2 class="section-title">开始搜索</h2>
            
            <form id="search-form" class="search-form">
              <!-- Keyword -->
              <div class="form-group">
                <label for="keyword" class="form-label">搜索关键词 <span class="required">*</span></label>
                <input 
                  type="text" 
                  id="keyword" 
                  name="keyword" 
                  class="form-input" 
                  placeholder="例如：machine learning rock strength prediction"
                  required
                >
              </div>
              
              <!-- Content Filter Help -->
              <div class="form-group">
                <label for="content_filter_help" class="form-label">研究方向描述</label>
                <textarea 
                  id="content_help" 
                  name="content_help" 
                  class="form-textarea" 
                  placeholder="描述您的研究方向，帮助AI更精准地筛选相关文献"
                  rows="3"
                ></textarea>
                <p class="form-hint">可选：提供研究方向描述可提高筛选精准度</p>
              </div>

              <!-- Source Selection -->
              <div class="form-group">
                <label class="form-label">检索来源</label>
                <div class="source-chip-group" id="source-chips" role="group" aria-label="选择检索来源">
                  <span class="source-loading">加载检索源...</span>
                </div>
                <p class="form-hint">至少选择一个来源；未选将使用服务端默认来源</p>
              </div>
              
              <!-- Year and Impact Factor Row -->
              <div class="form-row">
                <div class="form-group">
                  <label for="ylo" class="form-label">起始年份</label>
                  <input 
                    type="number" 
                    id="ylo" 
                    name="ylo" 
                    class="form-input" 
                    min="1900" 
                    max="2030"
                    value="${defaultYear}"
                  >
                  <p class="form-hint">默认：近5年</p>
                </div>
                
                <div class="form-group">
                  <label for="sciif" class="form-label">最低影响因子 (IF)</label>
                  <input 
                    type="number" 
                    id="sciif" 
                    name="sciif" 
                    class="form-input" 
                    step="0.1" 
                    min="0"
                    value="3"
                  >
                  <p class="form-hint">默认：3.0</p>
                </div>
              </div>
              
              <!-- Optional Filters -->
              <details class="optional-filters">
                <summary class="filters-toggle">
                  <span>更多筛选条件</span>
                  <svg class="toggle-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <polyline points="6 9 12 15 18 9"></polyline>
                  </svg>
                </summary>
                <div class="filters-content">
                  <div class="form-row">
                    <div class="form-group">
                      <label for="jci" class="form-label">最低 JCI 分数</label>
                      <input 
                        type="number" 
                        id="jci" 
                        name="jci" 
                        class="form-input" 
                        step="0.01" 
                        min="0"
                      >
                    </div>
                    
                    <div class="form-group">
                      <label for="sci" class="form-label">SCI 分区</label>
                      <select id="sci" name="sci" class="form-input">
                        <option value="">全部</option>
                        <option value="Q1">Q1</option>
                        <option value="Q2">Q2</option>
                        <option value="Q3">Q3</option>
                        <option value="Q4">Q4</option>
                      </select>
                    </div>
                  </div>
                </div>
              </details>
              
              <button type="submit" id="submit-btn" class="btn btn-primary">
                <svg class="btn-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                  <circle cx="11" cy="11" r="8"></circle>
                  <path d="m21 21-4.35-4.35"></path>
                </svg>
                <span>开始搜索</span>
              </button>
            </form>
          </div>
        </div>
      </section>

      <!-- Task Status Section -->
      <section id="task-section" class="task-section hidden">
        <div class="container">
          <div class="task-card">
            <div class="task-header">
              <h2 class="section-title">任务进度</h2>
              <span id="task-id" class="task-id"></span>
            </div>
            
            <div class="progress-container">
              <div class="progress-bar">
                <div id="progress-fill" class="progress-fill"></div>
              </div>
              <div class="progress-info">
                <span id="progress-step" class="progress-step">准备中...</span>
                <span id="progress-percent" class="progress-percent">0%</span>
              </div>
            </div>
            
            <div id="task-status" class="task-status">
              <div class="status-indicator status-queued">
                <span class="status-dot"></span>
                <span id="status-text">等待中</span>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- Results Section -->
      <section id="results-section" class="results-section hidden">
        <div class="container">
          <div class="results-card">
            <div class="results-header">
              <h2 class="section-title">搜索结果</h2>
              <div class="results-stats">
                <div class="stat-item">
                  <span class="stat-value" id="total-papers">0</span>
                  <span class="stat-label">总论文数</span>
                </div>
                <div class="stat-item stat-secondary">
                  <span class="stat-value" id="filtered-papers">0</span>
                  <span class="stat-label">筛选后</span>
                </div>
              </div>
            </div>
            
            <div class="download-buttons">
              <button id="download-csv" class="btn btn-secondary">
                <svg class="btn-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                  <polyline points="7 10 12 15 17 10"></polyline>
                  <line x1="12" y1="15" x2="12" y2="3"></line>
                </svg>
                <span>下载 CSV</span>
              </button>
              <button id="download-bibtex" class="btn btn-outline">
                <svg class="btn-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
                  <polyline points="14 2 14 8 20 8"></polyline>
                  <line x1="16" y1="13" x2="8" y2="13"></line>
                  <line x1="16" y1="17" x2="8" y2="17"></line>
                </svg>
                <span>导出 BibTeX</span>
              </button>
            </div>
            
            <div class="results-warning">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10"></circle>
                <line x1="12" y1="8" x2="12" y2="12"></line>
                <line x1="12" y1="16" x2="12.01" y2="16"></line>
              </svg>
              <span>搜索结果将在 10 分钟后自动清理，请及时下载所需数据</span>
            </div>
            
            <div id="results-table-container" class="results-table-container">
              <!-- Table will be inserted here -->
            </div>
          </div>
        </div>
      </section>

      <!-- Error Section -->
      <section id="error-section" class="error-section hidden">
        <div class="container">
          <div class="error-card">
            <div class="error-icon">
              <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10"></circle>
                <line x1="12" y1="8" x2="12" y2="12"></line>
                <line x1="12" y1="16" x2="12.01" y2="16"></line>
              </svg>
            </div>
            <h3 class="error-title">出错了</h3>
            <p id="error-message" class="error-message"></p>
            <button id="retry-btn" class="btn btn-primary">重试</button>
          </div>
        </div>
      </section>
    `;
  }

  mount() {
    // Form submission
    document.getElementById('search-form')?.addEventListener('submit', (e) => this.handleSubmit(e));
    document.getElementById('download-csv')?.addEventListener('click', () => this.handleDownloadCSV());
    document.getElementById('download-bibtex')?.addEventListener('click', () => this.handleDownloadBibTeX());
    document.getElementById('retry-btn')?.addEventListener('click', () => this.handleRetry());
    document.getElementById('history-toggle')?.addEventListener('click', () => window.toggleSidebar());

    // Dynamically load available sources from server config
    this.loadSources();
  }

  async loadSources() {
    const container = document.getElementById('source-chips');
    if (!container) return;
    try {
      const data = await fetchSources();
      if (!data.sources || data.sources.length === 0) {
        container.innerHTML = '<span class="source-loading">暂无可用检索源</span>';
        return;
      }
      container.innerHTML = data.sources.map((s, i) => `
        <label class="source-chip">
          <input type="checkbox" name="source_include" value="${s.id}"${i < 2 ? ' checked' : ''}>
          <span>${s.label}</span>
        </label>
      `).join('');
    } catch (e) {
      console.warn('Failed to load sources:', e);
      container.innerHTML = '<span class="source-loading">检索源加载失败</span>';
    }
  }

  async handleSubmit(e) {
    e.preventDefault();

    this.hideAllSections();
    this.setButtonLoading(true);

    const formData = new FormData(document.getElementById('search-form'));
    const params = this.buildRequestParams(formData);

    try {
      const response = await createTask(params);
      const taskId = response.task_id;
      this.currentTaskId = taskId;

      // Add to local history
      historyManager.addTask(taskId, params.keyword, 'queued');
      renderHistoryList();

      // Show task section
      this.showTaskSection(taskId);

      // Reset button immediately - don't block for polling
      this.setButtonLoading(false);

      // Poll for status in background (non-blocking)
      this.pollInBackground(taskId);

    } catch (error) {
      if (this.currentTaskId) {
        historyManager.updateTask(this.currentTaskId, { status: 'failed' });
        renderHistoryList();
      }
      this.showError(error.message);
      this.setButtonLoading(false);
    }
  }

  async pollInBackground(taskId) {
    try {
      const result = await pollTaskStatus(taskId, (status) => {
        // Only update UI if this is still the current task being viewed
        if (this.currentTaskId === taskId) {
          this.handleProgressUpdate(status);
        } else {
          // Still update history even if not viewing
          historyManager.updateTask(taskId, { status: status.status });
          renderHistoryList();
        }
      });

      // Update history
      historyManager.updateTask(taskId, { status: 'completed' });
      renderHistoryList();

      // Show results only if still viewing this task
      if (this.currentTaskId === taskId) {
        this.showResults(result);
      }

    } catch (error) {
      historyManager.updateTask(taskId, { status: 'failed' });
      renderHistoryList();

      // Show error only if still viewing this task
      if (this.currentTaskId === taskId) {
        this.showError(error.message);
      }
    }
  }

  buildRequestParams(formData) {
    const params = {
      keyword: formData.get('keyword'),
      enable_crossref: true,
    };

    const ylo = formData.get('ylo');
    if (ylo) params.ylo = parseInt(ylo, 10);

    const contentHelp = formData.get('content_help');
    if (contentHelp?.trim()) params.content_help = contentHelp.trim();

    const sciif = formData.get('sciif');
    if (sciif) params.sciif = parseFloat(sciif);

    const jci = formData.get('jci');
    if (jci?.trim()) params.jci = parseFloat(jci);

    const sci = formData.get('sci');
    if (sci?.trim()) params.sci = sci;

    const selectedSources = formData
      .getAll('source_include')
      .map((source) => String(source).trim())
      .filter(Boolean);
    if (selectedSources.length > 0) params.source_include = selectedSources;

    return params;
  }

  handleProgressUpdate(status) {
    const percent = status.progress?.percent || 0;
    document.getElementById('progress-fill').style.width = `${percent}%`;
    document.getElementById('progress-percent').textContent = `${percent}%`;

    const step = this.cleanStepText(status.progress?.step);
    document.getElementById('progress-step').textContent = step;

    this.updateStatusIndicator(status.status);

    // Update history with real backend status
    if (this.currentTaskId) {
      historyManager.updateTask(this.currentTaskId, { status: status.status });
      renderHistoryList();
    }
  }

  cleanStepText(step) {
    if (!step) return '处理中...';
    return step.replace(/\s*\([^)]*\)\s*/g, ' ').trim();
  }

  updateStatusIndicator(status) {
    const indicator = document.querySelector('#task-status .status-indicator');
    const statusText = document.getElementById('status-text');

    indicator.classList.remove('status-queued', 'status-running', 'status-completed', 'status-failed');

    const statusMap = {
      'queued': ['status-queued', '等待中'],
      'running': ['status-running', '运行中'],
      'completed': ['status-completed', '已完成'],
      'failed': ['status-failed', '失败']
    };

    const [cls, text] = statusMap[status] || ['status-queued', status];
    indicator.classList.add(cls);
    statusText.textContent = text;
  }

  showTaskSection(taskId) {
    document.getElementById('task-id').textContent = `ID: ${taskId}`;
    document.getElementById('task-section').classList.remove('hidden');
    document.getElementById('task-section').classList.add('fade-in');

    document.getElementById('progress-fill').style.width = '0%';
    document.getElementById('progress-percent').textContent = '0%';
    document.getElementById('progress-step').textContent = '准备中...';
    this.updateStatusIndicator('queued');
  }

  showResults(result) {
    const total = result.result?.total_papers || 0;
    const filtered = result.result?.filtered_papers || 0;

    document.getElementById('total-papers').textContent = total;
    document.getElementById('filtered-papers').textContent = filtered;

    this.currentPapers = result.result?.data || [];
    this.currentSortColumn = 'if_score';
    this.currentSortDirection = 'desc';
    this.sortPapers();
    this.buildResultsTable();

    document.getElementById('results-section').classList.remove('hidden');
    document.getElementById('results-section').classList.add('fade-in');
    document.getElementById('results-section').scrollIntoView({ behavior: 'smooth' });
  }

  sortPapers() {
    this.currentPapers.sort((a, b) => {
      let valA, valB;

      switch (this.currentSortColumn) {
        case 'title':
          valA = (a.title || '').toLowerCase();
          valB = (b.title || '').toLowerCase();
          break;
        case 'authors':
          valA = this.getAuthorsString(a.authors).toLowerCase();
          valB = this.getAuthorsString(b.authors).toLowerCase();
          break;
        case 'journal':
          valA = (a.journal || a.venue || '').toLowerCase();
          valB = (b.journal || b.venue || '').toLowerCase();
          break;
        case 'if_score':
          valA = parseFloat(a.if_score) || 0;
          valB = parseFloat(b.if_score) || 0;
          break;
        case 'year':
          valA = parseInt(a.year, 10) || 0;
          valB = parseInt(b.year, 10) || 0;
          break;
        default:
          return 0;
      }

      if (valA < valB) return this.currentSortDirection === 'asc' ? -1 : 1;
      if (valA > valB) return this.currentSortDirection === 'asc' ? 1 : -1;
      return 0;
    });
  }

  handleSort(column) {
    if (this.currentSortColumn === column) {
      this.currentSortDirection = this.currentSortDirection === 'asc' ? 'desc' : 'asc';
    } else {
      this.currentSortColumn = column;
      this.currentSortDirection = (column === 'if_score' || column === 'year') ? 'desc' : 'asc';
    }

    this.sortPapers();
    this.buildResultsTable();
  }

  getSortIndicator(column) {
    if (this.currentSortColumn !== column) return '';
    return this.currentSortDirection === 'asc' ? ' ↑' : ' ↓';
  }

  getAuthorsString(authors) {
    if (!authors) return '';
    if (typeof authors === 'string') return authors;
    if (Array.isArray(authors)) return authors.join(', ');
    return String(authors);
  }

  getJournalName(paper) {
    return paper.journal || paper.venue || paper.publicationVenue || paper.containerTitle || '';
  }

  getImpactFactor(paper) {
    const ifValue = paper.if_score || paper.sciif || paper.impactFactor || paper.if || paper.IF;
    if (ifValue && !isNaN(parseFloat(ifValue))) {
      return parseFloat(ifValue).toFixed(1);
    }
    return '-';
  }

  getImpactFactorValue(paper) {
    const ifValue = paper.if_score || paper.sciif || paper.impactFactor || paper.if || paper.IF;
    return parseFloat(ifValue) || 0;
  }

  getJournalColor(ifScore) {
    // Gradient from red (high IF) to blue (low IF)
    // High IF (>20): Deep red
    // Medium IF (10-20): Orange-red
    // Medium IF (5-10): Purple
    // Low IF (<5): Blue
    if (ifScore >= 20) {
      return 'background: linear-gradient(135deg, #DC2626, #EF4444); color: white;';
    } else if (ifScore >= 10) {
      return 'background: linear-gradient(135deg, #F97316, #FB923C); color: white;';
    } else if (ifScore >= 5) {
      return 'background: linear-gradient(135deg, #8B5CF6, #A78BFA); color: white;';
    } else if (ifScore >= 3) {
      return 'background: linear-gradient(135deg, #3B82F6, #60A5FA); color: white;';
    } else if (ifScore > 0) {
      return 'background: linear-gradient(135deg, #60A5FA, #93C5FD); color: #1E40AF;';
    } else {
      return 'background: #F3F4F6; color: #6B7280;';
    }
  }

  truncateText(text, maxLength) {
    if (!text) return '';
    if (text.length <= maxLength) return text;
    return text.substring(0, maxLength) + '...';
  }

  truncateAuthors(authors) {
    if (!authors) return '-';
    if (typeof authors === 'string') {
      const parts = authors.split(',');
      if (parts.length > 2) return parts.slice(0, 2).join(', ') + ' et al.';
      return authors;
    }
    if (Array.isArray(authors)) {
      if (authors.length > 2) return authors.slice(0, 2).join(', ') + ' et al.';
      return authors.join(', ');
    }
    return String(authors);
  }

  buildResultsTable() {
    const container = document.getElementById('results-table-container');

    if (!this.currentPapers || this.currentPapers.length === 0) {
      container.innerHTML = '<p style="text-align: center; color: #6B7280; padding: 2rem;">暂无结果</p>';
      return;
    }

    const table = document.createElement('table');
    table.className = 'results-table';

    table.innerHTML = `
      <thead>
        <tr>
          <th class="sortable" data-column="title">标题${this.getSortIndicator('title')}</th>
          <th class="sortable" data-column="authors">作者${this.getSortIndicator('authors')}</th>
          <th class="sortable" data-column="year">年份${this.getSortIndicator('year')}</th>
          <th class="sortable" data-column="journal">期刊${this.getSortIndicator('journal')}</th>
          <th class="sortable" data-column="if_score">IF${this.getSortIndicator('if_score')}</th>
          <th>PDF</th>
        </tr>
      </thead>
      <tbody>
        ${this.currentPapers.map(paper => {
      const ifScore = this.getImpactFactorValue(paper);
      const journalStyle = this.getJournalColor(ifScore);
      return `
          <tr>
            <td class="paper-title">
              ${paper.doi
          ? `<a href="https://doi.org/${paper.doi}" target="_blank" rel="noopener">${escapeHtml(paper.title || 'Untitled')}</a>`
          : escapeHtml(paper.title || 'Untitled')
        }
            </td>
            <td>${escapeHtml(this.truncateAuthors(paper.authors))}</td>
            <td>${paper.year || '-'}</td>
            <td><span class="journal-tag" style="${journalStyle}">${escapeHtml(this.getJournalName(paper)) || '-'}</span></td>
            <td class="if-value">${this.getImpactFactor(paper)}</td>
            <td class="pdf-cell">
              ${paper.pdf_url
          ? `<a href="${escapeHtml(paper.pdf_url)}" target="_blank" rel="noopener" class="pdf-link" title="下载 PDF">
                    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
                      <polyline points="14 2 14 8 20 8"></polyline>
                      <line x1="16" y1="13" x2="8" y2="13"></line>
                      <line x1="16" y1="17" x2="8" y2="17"></line>
                      <polyline points="10 9 9 9 8 9"></polyline>
                    </svg>
                  </a>`
          : '<span class="pdf-none">-</span>'
        }
            </td>
          </tr>
        `}).join('')}
      </tbody>
    `;

    container.innerHTML = '';
    container.appendChild(table);

    table.querySelectorAll('th.sortable').forEach(th => {
      th.addEventListener('click', () => this.handleSort(th.dataset.column));
    });
  }

  showError(message) {
    document.getElementById('error-message').textContent = message;
    document.getElementById('error-section').classList.remove('hidden');
    document.getElementById('error-section').classList.add('fade-in');
  }

  hideAllSections() {
    document.getElementById('task-section')?.classList.add('hidden');
    document.getElementById('results-section')?.classList.add('hidden');
    document.getElementById('error-section')?.classList.add('hidden');
  }

  setButtonLoading(isLoading) {
    const btn = document.getElementById('submit-btn');
    if (isLoading) {
      btn.disabled = true;
      btn.innerHTML = '<span class="spinner"></span><span>搜索中...</span>';
      btn.classList.add('btn-loading');
    } else {
      btn.disabled = false;
      btn.innerHTML = `
        <svg class="btn-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
          <circle cx="11" cy="11" r="8"></circle>
          <path d="m21 21-4.35-4.35"></path>
        </svg>
        <span>开始搜索</span>
      `;
      btn.classList.remove('btn-loading');
    }
  }

  handleDownloadCSV() {
    if (this.currentTaskId) downloadCSV(this.currentTaskId);
  }

  handleDownloadBibTeX() {
    if (this.currentTaskId) downloadBibTeX(this.currentTaskId);
  }

  handleRetry() {
    this.hideAllSections();
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }
}
