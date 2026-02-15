/**
 * RustScholar Frontend - Main Application with Client-Side Routing
 * Multi-page SPA with history sidebar
 */

import { createTask, pollTaskStatus, downloadCSV, downloadBibTeX, getTaskStatus } from './api/client.js';
import { HomePage } from './pages/home.js';
import { ApiPage } from './pages/api.js';
import { TaskPage } from './pages/task.js';
import { historyManager } from './utils/history.js';

// Router state
let currentPage = null;

/**
 * Simple client-side router
 */
class Router {
    constructor() {
        this.routes = {
            '/': HomePage,
            '/docs': ApiPage,  // Changed from /api to avoid proxy conflict
            '/task': TaskPage,
        };

        window.addEventListener('popstate', () => this.handleRoute());
    }

    navigate(path) {
        window.history.pushState({}, '', path);
        this.handleRoute();
    }

    handleRoute() {
        const path = window.location.pathname;
        const app = document.getElementById('app');

        // Check for task detail page
        if (path.startsWith('/task/')) {
            const taskId = path.split('/task/')[1];
            currentPage = new TaskPage(taskId);
        } else if (this.routes[path]) {
            currentPage = new this.routes[path]();
        } else {
            currentPage = new HomePage();
        }

        app.innerHTML = currentPage.render();
        currentPage.mount();
    }
}

// Global router instance
export const router = new Router();

/**
 * Initialize sidebar
 */
function initSidebar() {
    const sidebar = document.getElementById('sidebar');
    const overlay = document.getElementById('sidebar-overlay');
    const closeBtn = document.getElementById('sidebar-close');
    const clearBtn = document.getElementById('clear-history');

    // Toggle sidebar
    window.toggleSidebar = function (show) {
        if (show === undefined) {
            show = !sidebar.classList.contains('open');
        }
        sidebar.classList.toggle('open', show);
        overlay.classList.toggle('active', show);
    };

    // Close events
    closeBtn?.addEventListener('click', () => window.toggleSidebar(false));
    overlay?.addEventListener('click', () => window.toggleSidebar(false));

    // Clear history
    clearBtn?.addEventListener('click', () => {
        historyManager.clearHistory();
        renderHistoryList();
    });

    // Render history
    renderHistoryList();
}

/**
 * Render history list in sidebar
 */
export function renderHistoryList() {
    const container = document.getElementById('history-list');
    if (!container) return;

    const history = historyManager.getHistory();

    if (history.length === 0) {
        container.innerHTML = '<p class="history-empty">暂无查询历史</p>';
        return;
    }

    container.innerHTML = history.map(item => `
    <a href="/task/${item.taskId}" class="history-item" data-task-id="${item.taskId}">
      <div class="history-keyword">${escapeHtml(item.keyword)}</div>
      <div class="history-meta">
        <span class="history-status history-status-${item.status}">${getStatusText(item.status)}</span>
        <span class="history-time">${formatTime(item.createdAt)}</span>
      </div>
    </a>
  `).join('');

    // Add click handlers
    container.querySelectorAll('.history-item').forEach(el => {
        el.addEventListener('click', (e) => {
            e.preventDefault();
            window.toggleSidebar(false);
            router.navigate(`/task/${el.dataset.taskId}`);
        });
    });
}

function getStatusText(status) {
    const statusMap = {
        'queued': '等待中',
        'running': '运行中',
        'completed': '已完成',
        'failed': '失败'
    };
    return statusMap[status] || status;
}

function formatTime(timestamp) {
    const date = new Date(timestamp);
    const now = new Date();
    const diffMs = now - date;
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return '刚刚';
    if (diffMins < 60) return `${diffMins}分钟前`;
    if (diffHours < 24) return `${diffHours}小时前`;
    if (diffDays < 7) return `${diffDays}天前`;

    return date.toLocaleDateString('zh-CN');
}

function escapeHtml(text) {
    if (!text) return '';
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

/**
 * Initialize application
 */
function init() {
    initSidebar();
    router.handleRoute();
}

// Initialize on DOM ready
document.addEventListener('DOMContentLoaded', init);

// Export utilities for pages
export { escapeHtml, formatTime, getStatusText };
