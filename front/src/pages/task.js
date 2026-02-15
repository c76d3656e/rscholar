/**
 * Task Detail Page - View task status and results by ID
 */

import { getTaskStatus, downloadCSV, downloadBibTeX } from '../api/client.js';
import { router, escapeHtml } from '../main.js';
import { historyManager } from '../utils/history.js';

export class TaskPage {
  constructor(taskId) {
    this.taskId = taskId;
    this.taskData = null;
    this.currentPapers = [];
    this.currentSortColumn = 'if_score';
    this.currentSortDirection = 'desc';
    this.pollInterval = null;
  }

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
          <a href="/docs" class="header-link">API 文档</a>
        </div>
      </header>

      <!-- Task Detail Section -->
      <section class="task-detail-section">
        <div class="container">
          <div class="task-detail-card">
            <div class="task-detail-header">
              <a href="/" class="back-link">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <line x1="19" y1="12" x2="5" y2="12"></line>
                  <polyline points="12 19 5 12 12 5"></polyline>
                </svg>
                返回搜索
              </a>
              <h1 class="task-detail-title">任务详情</h1>
            </div>
            
            <div class="task-id-full">
              <span class="task-id-label">任务 ID</span>
              <code class="task-id-value">${this.taskId || '未知'}</code>
            </div>
            
            <!-- Loading State -->
            <div id="task-loading" class="task-loading">
              <div class="spinner-large"></div>
              <p>正在加载任务信息...</p>
            </div>
            
            <!-- Error State -->
            <div id="task-error" class="task-error hidden">
              <div class="error-icon">
                <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <circle cx="12" cy="12" r="10"></circle>
                  <line x1="12" y1="8" x2="12" y2="12"></line>
                  <line x1="12" y1="16" x2="12.01" y2="16"></line>
                </svg>
              </div>
              <p id="task-error-message">任务不存在或已过期</p>
              <button id="retry-fetch" class="btn btn-primary">重试</button>
            </div>
            
            <!-- Task Content -->
            <div id="task-content" class="task-content hidden">
              <!-- Status Card (hidden when completed) -->
              <div id="status-card-wrapper" class="status-card">
                <div class="status-row">
                  <span class="status-label">状态</span>
                  <div id="status-indicator" class="status-indicator status-queued">
                    <span class="status-dot"></span>
                    <span id="status-text">等待中</span>
                  </div>
                </div>
                
                <div id="progress-container" class="progress-container">
                  <div class="progress-bar">
                    <div id="progress-fill" class="progress-fill"></div>
                  </div>
                  <div class="progress-info">
                    <span id="progress-step" class="progress-step">-</span>
                    <span id="progress-percent" class="progress-percent">0%</span>
                  </div>
                </div>
              </div>
              
              <!-- Results Section (shown when completed) -->
              <div id="task-results" class="task-results hidden">
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
                  <div id="source-stats" class="source-stats hidden"></div>
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
                    </svg>
                    <span>导出 BibTeX</span>
                  </button>
                </div>
                
                <div id="results-table-container" class="results-table-container">
                  <!-- Table will be inserted here -->
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>
    `;
  }

  mount() {
    document.getElementById('history-toggle')?.addEventListener('click', () => window.toggleSidebar());
    document.getElementById('retry-fetch')?.addEventListener('click', () => this.fetchTaskStatus());
    document.getElementById('download-csv')?.addEventListener('click', () => downloadCSV(this.taskId));
    document.getElementById('download-bibtex')?.addEventListener('click', () => downloadBibTeX(this.taskId));

    // Handle internal links
    document.querySelectorAll('a[href^="/"]').forEach(link => {
      link.addEventListener('click', (e) => {
        e.preventDefault();
        if (this.pollInterval) clearInterval(this.pollInterval);
        router.navigate(link.getAttribute('href'));
      });
    });

    // Fetch task status
    if (this.taskId) {
      this.fetchTaskStatus();
    } else {
      this.showError('任务 ID 无效');
    }
  }

  async fetchTaskStatus() {
    this.showLoading();

    try {
      const data = await getTaskStatus(this.taskId);
      this.taskData = data;
      this.showContent(data);

      // If still running, poll for updates
      if (data.status === 'queued' || data.status === 'running') {
        this.startPolling();
      }

      // Update local history if exists
      historyManager.updateTask(this.taskId, { status: data.status });

    } catch (error) {
      this.showError(error.message || '无法获取任务信息');
    }
  }

  startPolling() {
    if (this.pollInterval) clearInterval(this.pollInterval);

    this.pollInterval = setInterval(async () => {
      try {
        const data = await getTaskStatus(this.taskId);
        this.taskData = data;
        this.updateContent(data);

        if (data.status === 'completed' || data.status === 'failed') {
          clearInterval(this.pollInterval);
          this.pollInterval = null;
          historyManager.updateTask(this.taskId, { status: data.status });
        }
      } catch (error) {
        console.error('Polling error:', error);
      }
    }, 2000);
  }

  showLoading() {
    document.getElementById('task-loading')?.classList.remove('hidden');
    document.getElementById('task-error')?.classList.add('hidden');
    document.getElementById('task-content')?.classList.add('hidden');
  }

  showError(message) {
    document.getElementById('task-loading')?.classList.add('hidden');
    document.getElementById('task-error')?.classList.remove('hidden');
    document.getElementById('task-content')?.classList.add('hidden');
    document.getElementById('task-error-message').textContent = message;
  }

  showContent(data) {
    document.getElementById('task-loading')?.classList.add('hidden');
    document.getElementById('task-error')?.classList.add('hidden');
    document.getElementById('task-content')?.classList.remove('hidden');

    this.updateContent(data);
  }

  updateContent(data) {
    // Update status
    const indicator = document.getElementById('status-indicator');
    const statusText = document.getElementById('status-text');

    indicator.classList.remove('status-queued', 'status-running', 'status-completed', 'status-failed');

    const statusMap = {
      'queued': ['status-queued', '等待中'],
      'running': ['status-running', '运行中'],
      'completed': ['status-completed', '已完成'],
      'failed': ['status-failed', '失败']
    };

    const [cls, text] = statusMap[data.status] || ['status-queued', data.status];
    indicator.classList.add(cls);
    statusText.textContent = text;

    // Update progress
    const percent = data.progress?.percent || 0;
    document.getElementById('progress-fill').style.width = `${percent}%`;
    document.getElementById('progress-percent').textContent = `${percent}%`;
    document.getElementById('progress-step').textContent = this.cleanStepText(data.progress?.step) || '-';

    // Show results if completed
    if (data.status === 'completed' && data.result) {
      this.showResults(data.result);
    }
  }

  cleanStepText(step) {
    if (!step) return '';
    return step.replace(/\s*\([^)]*\)\s*/g, ' ').trim();
  }

  showResults(result) {
    // Hide status card when showing results
    document.getElementById('status-card-wrapper')?.classList.add('hidden');

    document.getElementById('task-results')?.classList.remove('hidden');

    document.getElementById('total-papers').textContent = result.total_papers || 0;
    document.getElementById('filtered-papers').textContent = result.filtered_papers || 0;

    // Render per-source counts and errors
    this.renderSourceStats(result.source_counts, result.source_errors);

    this.currentPapers = result.data || [];
    this.sortPapers();
    this.buildResultsTable();
  }

  renderSourceStats(sourceCounts, sourceErrors) {
    const container = document.getElementById('source-stats');
    if (!container) return;

    const items = [];

    // Source counts
    if (sourceCounts && typeof sourceCounts === 'object') {
      for (const [source, count] of Object.entries(sourceCounts)) {
        const hasError = sourceErrors && sourceErrors[source];
        const label = this.getSourceLabel(source);
        if (hasError) {
          items.push(`<span class="source-stat source-stat-error" title="${escapeHtml(sourceErrors[source])}">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path>
              <line x1="12" y1="9" x2="12" y2="13"></line>
              <line x1="12" y1="17" x2="12.01" y2="17"></line>
            </svg>
            ${label}: ${count}
          </span>`);
        } else {
          items.push(`<span class="source-stat">${label}: ${count}</span>`);
        }
      }
    }

    // Sources with errors but no counts (shouldn't happen, but be safe)
    if (sourceErrors && typeof sourceErrors === 'object') {
      for (const [source, err] of Object.entries(sourceErrors)) {
        if (sourceCounts && sourceCounts[source] !== undefined) continue;
        const label = this.getSourceLabel(source);
        items.push(`<span class="source-stat source-stat-error" title="${escapeHtml(err)}">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path>
            <line x1="12" y1="9" x2="12" y2="13"></line>
            <line x1="12" y1="17" x2="12.01" y2="17"></line>
          </svg>
          ${label}: 失败
        </span>`);
      }
    }

    if (items.length > 0) {
      container.innerHTML = items.join('<span class="source-stat-sep">·</span>');
      container.classList.remove('hidden');
    } else {
      container.classList.add('hidden');
    }
  }

  getSourceLabel(source) {
    const labels = {
      'openalex': 'OpenAlex',
      'semanticscholar': 'Semantic Scholar',
      'pubmed': 'PubMed',
      'arxiv': 'arXiv',
      'biorxiv': 'bioRxiv',
      'medrxiv': 'medRxiv',
    };
    return labels[source] || source;
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
      this.currentSortDirection = column === 'if_score' ? 'desc' : 'asc';
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
    return paper.journal || paper.venue || paper.publicationVenue || '';
  }

  getImpactFactor(paper) {
    const ifValue = paper.if_score || paper.sciif || paper.impactFactor || paper.if;
    if (ifValue && !isNaN(parseFloat(ifValue))) {
      return parseFloat(ifValue).toFixed(1);
    }
    return '-';
  }

  getImpactFactorValue(paper) {
    const ifValue = paper.if_score || paper.sciif || paper.impactFactor || paper.if;
    return parseFloat(ifValue) || 0;
  }

  getJournalColor(ifScore) {
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
          <th>年份</th>
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
}
