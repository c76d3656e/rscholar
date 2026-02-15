/**
 * RustScholar API Client
 * Handles all communication with the backend through the proxy
 */

const API_BASE = '';

/**
 * Create a search task
 * @param {Object} params - Search parameters
 * @returns {Promise<Object>} Task creation response
 */
export async function createTask(params) {
    const response = await fetch(`${API_BASE}/tasks`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(params),
    });

    if (!response.ok) {
        const error = await response.text();
        throw new Error(`创建任务失败: ${error}`);
    }

    return response.json();
}

/**
 * Get task status
 * @param {string} taskId - Task ID to check
 * @returns {Promise<Object>} Task status response
 */
export async function getTaskStatus(taskId) {
    const response = await fetch(`${API_BASE}/tasks/${taskId}`);

    if (!response.ok) {
        const error = await response.text();
        throw new Error(`获取任务状态失败: ${error}`);
    }

    return response.json();
}

/**
 * Download CSV results
 * @param {string} taskId - Task ID
 */
export function downloadCSV(taskId) {
    const link = document.createElement('a');
    link.href = `${API_BASE}/tasks/${taskId}/download`;
    link.download = `rustscholar_${taskId}.csv`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
}

/**
 * Download BibTeX results
 * @param {string} taskId - Task ID
 */
export function downloadBibTeX(taskId) {
    const link = document.createElement('a');
    link.href = `${API_BASE}/tasks/${taskId}/bibtex`;
    link.download = `rustscholar_${taskId}.bib`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
}

/**
 * Check API health
 * @returns {Promise<Object>} Health status
 */
export async function checkHealth() {
    const response = await fetch(`${API_BASE}/health`);

    if (!response.ok) {
        throw new Error('服务不可用');
    }

    return response.json();
}

/**
 * Fetch enabled search sources from server config
 * @returns {Promise<Object>} Sources response { sources: [{id, label}, ...] }
 */
export async function fetchSources() {
    const response = await fetch(`${API_BASE}/sources`);

    if (!response.ok) {
        throw new Error('获取检索源失败');
    }

    return response.json();
}

/**
 * Poll task status until completion
 * @param {string} taskId - Task ID
 * @param {Function} onProgress - Progress callback
 * @param {number} interval - Poll interval in ms
 * @returns {Promise<Object>} Final task result
 */
export async function pollTaskStatus(taskId, onProgress, interval = 2000) {
    return new Promise((resolve, reject) => {
        let consecutiveErrors = 0;
        const maxRetries = 5;

        const poll = async () => {
            try {
                const status = await getTaskStatus(taskId);
                consecutiveErrors = 0; // Reset on success

                if (onProgress) {
                    onProgress(status);
                }

                if (status.status === 'completed') {
                    resolve(status);
                } else if (status.status === 'failed') {
                    reject(new Error(status.error || '任务执行失败'));
                } else {
                    // Continue polling for 'queued' or 'running' status
                    setTimeout(poll, interval);
                }
            } catch (error) {
                consecutiveErrors++;
                console.warn(`Polling error (${consecutiveErrors}/${maxRetries}):`, error.message);

                if (consecutiveErrors >= maxRetries) {
                    reject(new Error('网络连接失败，请检查网络后刷新页面'));
                } else {
                    // Retry after a delay
                    setTimeout(poll, interval * 2);
                }
            }
        };

        poll();
    });
}
