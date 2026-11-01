export class ApiFailure extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (response.status === 204) {
    return undefined as T;
  }
  const text = await response.text();
  let payload: any = null;
  try {
    payload = text ? JSON.parse(text) : null;
  } catch {
    throw new ApiFailure(response.status, "invalid_response", `サーバーの応答を解釈できません (${response.status})`);
  }
  if (!response.ok) {
    throw new ApiFailure(
      response.status,
      payload?.error?.code ?? "unknown_error",
      payload?.error?.message ?? `リクエストに失敗しました (${response.status})`,
    );
  }
  return payload as T;
}

export const api = {
  get: <T>(path: string) => call<T>("GET", path),
  post: <T>(path: string, body: unknown = {}) => call<T>("POST", path, body),
  patch: <T>(path: string, body: unknown) => call<T>("PATCH", path, body),
  put: <T>(path: string, body: unknown) => call<T>("PUT", path, body),
  delete: <T>(path: string) => call<T>("DELETE", path),
};

export async function uploadFile(taskKey: string, file: File, role: string): Promise<void> {
  const query = new URLSearchParams({ role, name: file.name });
  const response = await fetch(`/api/v1/tasks/${taskKey}/files?${query}`, {
    method: "POST",
    headers: { "Content-Type": file.type || "application/octet-stream" },
    body: file,
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => null);
    throw new ApiFailure(
      response.status,
      payload?.error?.code ?? "unknown_error",
      payload?.error?.message ?? "アップロードに失敗しました",
    );
  }
}
