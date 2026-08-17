export interface Plan {
  actions: any[];
  skipped: { file: string; reason: string }[];
}

export interface JobStatus {
  id: string;
  kind: string;
  state: string; // 'pending' | 'running' | 'completed' | 'failed'
  progress: number;
  total: number;
}

export interface DateRepairParams {
  paths: string[];
  mode: {
    Auto?: any;
    Manual?: string;
    Shift?: string;
    Sidecar?: any;
  };
}

export interface ApiClient {
  f1_plan(params: DateRepairParams): Promise<Plan>;
  f1_apply(plan: Plan): Promise<void>;
  // We add other tools as needed
}

export class HttpApiClient implements ApiClient {
  private baseUrl: string;
  private getToken: () => Promise<string | null>;

  constructor(baseUrl: string, getToken: () => Promise<string | null>) {
    this.baseUrl = baseUrl;
    this.getToken = getToken;
  }

  private async fetchWithAuth(path: string, options: RequestInit = {}): Promise<Response> {
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
      } else {
        throw new Error(body.message || 'Unauthorized');
      }
    }

    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({ error: 'Unknown API error' }));
      throw new Error(errorBody.error || response.statusText);
    }

    return response;
  }

  async f1_plan(params: DateRepairParams): Promise<Plan> {
    const res = await this.fetchWithAuth('/api/tools/f1/plan', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(params),
    });
    return res.json();
  }

  async f1_apply(plan: Plan): Promise<void> {
    await this.fetchWithAuth('/api/tools/f1/apply', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(plan),
    });
  }
}
