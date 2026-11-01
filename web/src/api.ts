export class ApiFailure extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

/** サーバーのエラーコードごとの利用者向けの説明。未知のコードはサーバーの文面をそのまま出す。 */
const ERROR_MESSAGES: Record<string, string> = {
  invalid_project_key: "キーは英大文字で始まる1〜10文字の英大文字・数字にしてください",
  duplicate_project_key: "このキーはすでに使われています",
  permission_denied: "この操作を行う権限がありません",
  task_occupied: "作業者が完了または解放するまで、この操作はできません",
  version_conflict: "他の人が先に変更しました。最新の内容を確認してください",
  transition_condition_unmet: "この遷移には現段階の作業完了が必要です",
  stale_claim: "この作業権はすでに無効です",
  not_claimable: "現在この仕事は取得できません",
  dependency_cycle: "依存関係が循環します",
  dependency_exists: "この依存関係はすでにあります",
  stage_in_use: "この段階にタスクが残っているため削除できません",
  last_admin: "組織には有効な人間の管理者が1人以上必要です",
  email_taken: "このメールアドレスは登録済みです",
  signup_link_invalid: "このリンクは無効か、期限切れ、または使用済みです",
  too_many_attempts: "ログインの失敗が続いたため、しばらく時間をおいてください",
  unauthenticated: "メールアドレスまたはパスワードが正しくありません",
};

function errorMessage(code: string | undefined, fallback: string): string {
  return (code && ERROR_MESSAGES[code]) || fallback;
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
      errorMessage(payload?.error?.code, payload?.error?.message ?? `リクエストに失敗しました (${response.status})`),
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
      errorMessage(payload?.error?.code, payload?.error?.message ?? "アップロードに失敗しました"),
    );
  }
}
