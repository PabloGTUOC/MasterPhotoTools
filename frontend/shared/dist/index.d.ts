export interface Plan {
    actions: any[];
    skipped: {
        file: string;
        reason: string;
    }[];
}
export interface JobStatus {
    id: string;
    kind: string;
    state: string;
    progress: number;
    total: number;
}
export interface DateRepairParams {
    paths: string[];
    mode: {
        Auto?: any;
        Manual?: string;
        Shift?: number;
        Sidecar?: any;
    };
}
export interface ApiClient {
    f1_plan(params: DateRepairParams): Promise<Plan>;
    f1_apply(plan: Plan): Promise<void>;
}
export declare class HttpApiClient implements ApiClient {
    private baseUrl;
    private getToken;
    constructor(baseUrl: string, getToken: () => Promise<string | null>);
    private fetchWithAuth;
    f1_plan(params: DateRepairParams): Promise<Plan>;
    f1_apply(plan: Plan): Promise<void>;
}
