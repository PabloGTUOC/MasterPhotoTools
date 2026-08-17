"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.HttpApiClient = void 0;
class HttpApiClient {
    baseUrl;
    getToken;
    constructor(baseUrl, getToken) {
        this.baseUrl = baseUrl;
        this.getToken = getToken;
    }
    async fetchWithAuth(path, options = {}) {
        const token = await this.getToken();
        const headers = new Headers(options.headers || {});
        if (token) {
            headers.set('Authorization', `Bearer ${token}`);
        }
        let response = await fetch(`${this.baseUrl}${path}`, {
            ...options,
            headers,
        });
        if (response.status === 401) {
            const body = await response.json().catch(() => ({}));
            if (body.code === 'token_expired') {
                // Trigger a force refresh and retry
                const newToken = await this.getToken(); // in real app, we pass a forceRefresh flag
                if (newToken) {
                    headers.set('Authorization', `Bearer ${newToken}`);
                    response = await fetch(`${this.baseUrl}${path}`, {
                        ...options,
                        headers,
                    });
                }
            }
            else {
                throw new Error(body.message || 'Unauthorized');
            }
        }
        if (!response.ok) {
            const errorBody = await response.json().catch(() => ({ error: 'Unknown API error' }));
            throw new Error(errorBody.error || response.statusText);
        }
        return response;
    }
    async f1_plan(params) {
        const res = await this.fetchWithAuth('/api/tools/f1/plan', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(params),
        });
        return res.json();
    }
    async f1_apply(plan) {
        await this.fetchWithAuth('/api/tools/f1/apply', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(plan),
        });
    }
}
exports.HttpApiClient = HttpApiClient;
