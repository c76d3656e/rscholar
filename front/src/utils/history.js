/**
 * History Manager - Local Storage for Query History
 */

const STORAGE_KEY = 'rustscholar_history';
const MAX_HISTORY_ITEMS = 50;

class HistoryManager {
    constructor() {
        this.history = this.loadHistory();
    }

    loadHistory() {
        try {
            const data = localStorage.getItem(STORAGE_KEY);
            return data ? JSON.parse(data) : [];
        } catch (e) {
            console.error('Failed to load history:', e);
            return [];
        }
    }

    saveHistory() {
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(this.history));
        } catch (e) {
            console.error('Failed to save history:', e);
        }
    }

    addTask(taskId, keyword, status = 'queued') {
        // Check if task already exists
        const existing = this.history.findIndex(item => item.taskId === taskId);
        if (existing >= 0) {
            this.history[existing].status = status;
            this.history[existing].updatedAt = Date.now();
        } else {
            this.history.unshift({
                taskId,
                keyword,
                status,
                createdAt: Date.now(),
                updatedAt: Date.now(),
            });
        }

        // Limit history size
        if (this.history.length > MAX_HISTORY_ITEMS) {
            this.history = this.history.slice(0, MAX_HISTORY_ITEMS);
        }

        this.saveHistory();
    }

    updateTask(taskId, updates) {
        const item = this.history.find(item => item.taskId === taskId);
        if (item) {
            Object.assign(item, updates, { updatedAt: Date.now() });
            this.saveHistory();
        }
    }

    getTask(taskId) {
        return this.history.find(item => item.taskId === taskId);
    }

    getHistory() {
        return this.history;
    }

    clearHistory() {
        this.history = [];
        this.saveHistory();
    }

    removeTask(taskId) {
        this.history = this.history.filter(item => item.taskId !== taskId);
        this.saveHistory();
    }
}

export const historyManager = new HistoryManager();
