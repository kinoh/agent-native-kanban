import { type FormEvent, useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Session } from "../App";
import type { ApiToken, Permission, Principal, SignupLink } from "../types";
import { Avatar, ErrorNote, Modal, formatTime, shortTokenId } from "../ui";

const PERMISSIONS: Permission[] = ["view", "create", "edit", "claim", "assign", "review", "approve", "admin"];

const LIFETIMES: [string, number][] = [
  ["1時間", 3600],
  ["1日", 86400],
  ["7日", 7 * 86400],
  ["30日", 30 * 86400],
];

interface Grant {
  id: string;
  project_id: string | null;
  permission: Permission;
}

type TokenState = "有効" | "失効済み" | "期限切れ" | "親トークンが無効";

function ownState(token: ApiToken, now: string): TokenState {
  if (token.revoked_at) return "失効済み";
  return token.expires_at !== null && token.expires_at <= now ? "期限切れ" : "有効";
}

/** 親の失効は子の `revoked_at` に現れないので、親をたどって実際の状態を出す。 */
function tokenState(token: ApiToken, tokens: Map<string, ApiToken>, now: string): TokenState {
  const own = ownState(token, now);
  if (own !== "有効") return own;
  let parent = token.parent_token_id ? tokens.get(token.parent_token_id) : undefined;
  while (parent) {
    if (ownState(parent, now) !== "有効") return "親トークンが無効";
    parent = parent.parent_token_id ? tokens.get(parent.parent_token_id) : undefined;
  }
  return "有効";
}

function delegationRange(token: ApiToken, projectName: (id: string) => string): string {
  const byProject = new Map<string, Permission[]>();
  for (const scope of token.scopes ?? []) {
    byProject.set(scope.project_id, [...(byProject.get(scope.project_id) ?? []), scope.permission]);
  }
  const scopes = [...byProject].map(([id, permissions]) => `${projectName(id)}: ${permissions.join(", ")}`);
  const keys = token.task_keys ?? [];
  const hidden = (token.tasks?.length ?? 0) - keys.length;
  const tasks = [...keys, ...(hidden > 0 ? [`ほか${hidden}件`] : [])];
  return `${scopes.join(" / ")}（${tasks.length > 0 ? `タスク ${tasks.join(", ")} と子孫` : "プロジェクト全体"}）`;
}

function GrantEditor({
  session,
  principal,
  onClose,
}: {
  session: Session;
  principal: Principal;
  onClose: () => void;
}) {
  const [grants, setGrants] = useState<Grant[]>([]);
  const [tokens, setTokens] = useState<ApiToken[]>([]);
  const [project, setProject] = useState("");
  const [permission, setPermission] = useState<Permission>("view");
  const [token, setToken] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const path = `/principals/${principal.id}`;

  const reload = useCallback(async () => {
    const [granted, issued] = await Promise.all([
      api.get<{ grants: Grant[] }>(`${path}/grants`),
      api.get<{ tokens: ApiToken[] }>(`${path}/tokens`),
    ]);
    setGrants(granted.grants);
    setTokens(issued.tokens);
  }, [path]);
  useEffect(() => {
    reload().catch((failure: Error) => setError(failure.message));
  }, [reload]);

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
      await reload();
      setError(null);
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  const projectName = (id: string | null) =>
    id === null ? "組織全体" : session.projects.find((item) => item.id === id)?.name ?? id;

  const now = new Date().toISOString();
  const tokensById = new Map(tokens.map((token) => [token.id, token]));

  return (
    <Modal title={`${principal.display_name} の権限と認証情報`} onClose={onClose}>
      <ul className="related">
        {grants.map((grant) => (
          <li key={grant.id}>
            {projectName(grant.project_id)}: {grant.permission}
            <button className="link" onClick={() => void run(() => api.delete(`/grants/${grant.id}`))}>外す</button>
          </li>
        ))}
      </ul>
      <form
        className="inline-form"
        onSubmit={(event) => {
          event.preventDefault();
          void run(() => api.post(`${path}/grants`, { project_id: project || null, permission }));
        }}
      >
        <select value={project} onChange={(e) => setProject(e.target.value)} aria-label="範囲">
          <option value="">組織全体</option>
          {session.projects.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select>
        <select value={permission} onChange={(e) => setPermission(e.target.value as Permission)} aria-label="権限">
          {PERMISSIONS.map((value) => <option key={value}>{value}</option>)}
        </select>
        <button>権限を追加</button>
      </form>
      {principal.kind === "human" && (
        <form
          className="inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            void run(() => api.put(`${path}/password`, { password })).then(() => setPassword(""));
          }}
        >
          <input
            type="password"
            autoComplete="new-password"
            minLength={12}
            placeholder="新しいパスワード（12文字以上）"
            aria-label="新しいパスワード"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
          <button>パスワードを再設定</button>
        </form>
      )}
      <h3>API トークン</h3>
      <button
        onClick={() =>
          void run(async () => {
            const reply = await api.post<{ token: string }>(`${path}/tokens`, { name: `issued ${new Date().toISOString()}` });
            setToken(reply.token);
          })
        }
      >
        トークンを発行
      </button>
      {token && (
        <div className="secret">
          <p>このトークンは今だけ表示されます。</p>
          <code>{token}</code>
        </div>
      )}
      <ul className="token-list" aria-label="発行済みのトークン">
        {tokens.map((item) => {
          const state = tokenState(item, tokensById, now);
          const parent = item.parent_token_id ? tokensById.get(item.parent_token_id) : undefined;
          return (
            <li key={item.id}>
              <div className="token-head">
                <span>{item.name}</span>
                {item.parent_token_id && <span className="badge">委譲 {shortTokenId(item.id)}</span>}
                <span className="muted token-state">{state}</span>
                {state === "有効" && (
                  <button
                    className="link"
                    onClick={() => {
                      if (window.confirm(`${item.name} を失効しますか？ここから委譲したトークンも無効になります。`)) {
                        void run(() => api.delete(`/tokens/${item.id}`));
                      }
                    }}
                  >
                    失効
                  </button>
                )}
              </div>
              <p className="muted">
                期限: {item.expires_at ? formatTime(item.expires_at) : "なし"}
                {parent && ` / 委譲元: ${parent.name}`}
              </p>
              {item.parent_token_id && <p className="muted">範囲: {delegationRange(item, projectName)}</p>}
            </li>
          );
        })}
      </ul>
      <ErrorNote message={error} />
    </Modal>
  );
}

function RenamePrincipal({
  principal,
  onClose,
  onSaved,
}: {
  principal: Principal;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const [name, setName] = useState(principal.display_name);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) {
      setError("表示名を入力してください");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await api.patch(`/principals/${principal.id}`, { display_name: name.trim() });
      await onSaved();
      onClose();
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal title="表示名を変更" onClose={onClose}>
      <form onSubmit={(event) => void save(event)}>
        <label>
          表示名
          <input value={name} maxLength={100} onChange={(event) => setName(event.target.value)} required />
        </label>
        <div className="actions">
          <button className="primary" disabled={saving}>{saving ? "保存中" : "表示名を保存"}</button>
          <button type="button" onClick={onClose} disabled={saving}>キャンセル</button>
        </div>
        <ErrorNote message={error} />
      </form>
    </Modal>
  );
}

function OrganizationName({ session }: { session: Session }) {
  const [name, setName] = useState(session.me.organization.name);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => setName(session.me.organization.name), [session.me.organization.name]);

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) {
      setError("組織名を入力してください");
      return;
    }
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      await api.patch("/organization", { name: name.trim() });
      await session.reloadMe();
      setSaved(true);
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <section>
      <h2>組織名</h2>
      <form className="inline-form" onSubmit={(event) => void save(event)}>
        <input aria-label="組織名" value={name} maxLength={100} onChange={(event) => setName(event.target.value)} required />
        <button className="primary" disabled={saving}>{saving ? "保存中" : "組織名を保存"}</button>
      </form>
      {saved && <p className="success-note" role="status">保存しました</p>}
      <ErrorNote message={error} />
    </section>
  );
}

export function Admin({ session }: { session: Session }) {
  const [principals, setPrincipals] = useState<Principal[]>([]);
  const [links, setLinks] = useState<SignupLink[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [agentName, setAgentName] = useState("");
  const [note, setNote] = useState("");
  const [lifetime, setLifetime] = useState(LIFETIMES[2][1]);
  const [maxUses, setMaxUses] = useState(10);
  const [grantProject, setGrantProject] = useState("");
  const [issued, setIssued] = useState<string | null>(null);
  const [editing, setEditing] = useState<Principal | null>(null);
  const [renaming, setRenaming] = useState<Principal | null>(null);

  const reload = useCallback(async () => {
    try {
      const [people, signup] = await Promise.all([
        api.get<{ principals: Principal[] }>("/principals"),
        api.get<{ signup_links: SignupLink[] }>("/signup-links"),
      ]);
      setPrincipals(people.principals);
      setLinks(signup.signup_links);
    } catch (failure) {
      setError((failure as Error).message);
      throw failure;
    }
  }, []);
  useEffect(() => {
    void reload().catch(() => undefined);
  }, [reload]);

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
      await reload();
      setError(null);
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  const createAgent = (event: FormEvent) => {
    event.preventDefault();
    void run(() => api.post("/principals", { display_name: agentName })).then(() => setAgentName(""));
  };

  const createLink = (event: FormEvent) => {
    event.preventDefault();
    const permissions: Permission[] = ["view", "create", "edit", "claim", "assign", "review"];
    void run(async () => {
      const reply = await api.post<{ url: string }>("/signup-links", {
        note,
        expires_in_seconds: lifetime,
        max_uses: maxUses,
        grants: permissions.map((permission) => ({ project_id: grantProject || null, permission })),
      });
      setIssued(reply.url);
      setNote("");
    });
  };

  const now = new Date().toISOString();
  return (
    <div className="page">
      <header className="page-header">
        <h1>組織の管理</h1>
      </header>
      <ErrorNote message={error} />

      <OrganizationName session={session} />

      <h2>サインアップリンク</h2>
      <p className="muted">
        リンクを受け取った人が自分で登録します。登録すると、選んだ範囲で閲覧・作成・編集・取得・担当変更・レビューの権限を持ちます。
      </p>
      <form className="inline-form" onSubmit={createLink}>
        <input placeholder="用途（例: 開発チーム）" value={note} onChange={(e) => setNote(e.target.value)} required />
        <select value={lifetime} onChange={(e) => setLifetime(Number(e.target.value))} aria-label="有効期間">
          {LIFETIMES.map(([label, seconds]) => <option key={seconds} value={seconds}>有効期間 {label}</option>)}
        </select>
        <label className="check">
          上限
          <input type="number" min={1} value={maxUses} onChange={(e) => setMaxUses(Number(e.target.value))} aria-label="利用回数の上限" />
          回
        </label>
        <select value={grantProject} onChange={(e) => setGrantProject(e.target.value)} aria-label="権限の範囲">
          <option value="">組織全体</option>
          {session.projects.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select>
        <button className="primary">リンクを発行</button>
      </form>
      {issued && (
        <div className="secret">
          <p>この URL は今だけ表示されます。</p>
          <code>{issued}</code>
        </div>
      )}
      <table className="settings-table">
        <thead>
          <tr><th>用途</th><th>利用</th><th>期限</th><th>状態</th><th /></tr>
        </thead>
        <tbody>
          {links.map((link) => {
            const active = !link.revoked_at && link.expires_at > now && link.used_count < link.max_uses;
            return (
              <tr key={link.id}>
                <td>{link.note}</td>
                <td>{link.used_count} / {link.max_uses}</td>
                <td>{formatTime(link.expires_at)}</td>
                <td>{active ? "有効" : "無効"}</td>
                <td>
                  {active && <button className="link" onClick={() => void run(() => api.delete(`/signup-links/${link.id}`))}>無効化</button>}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      <h2>主体</h2>
      <form className="inline-form" onSubmit={createAgent}>
        <input placeholder="エージェント名" value={agentName} onChange={(e) => setAgentName(e.target.value)} required />
        <button>エージェントを登録</button>
      </form>
      <table className="settings-table">
        <thead>
          <tr><th>名前</th><th>種別</th><th>メール</th><th>状態</th><th /></tr>
        </thead>
        <tbody>
          {principals.map((principal) => (
            <tr key={principal.id}>
              <td className="principal"><Avatar principal={principal} /> {principal.display_name}</td>
              <td>{principal.kind === "agent" ? "エージェント" : "人間"}</td>
              <td>{principal.email ?? "—"}</td>
              <td>{{ active: "有効", disabled: "無効", deleted: "削除済み" }[principal.status]}</td>
              <td>
                {principal.status !== "deleted" && (
                  <>
                    <button className="link" onClick={() => setRenaming(principal)}>表示名を変更</button>
                    <button className="link" onClick={() => setEditing(principal)}>権限</button>
                    <button
                      className="link"
                      onClick={() =>
                        void run(() =>
                          api.patch(`/principals/${principal.id}`, {
                            status: principal.status === "active" ? "disabled" : "active",
                          }),
                        )
                      }
                    >
                      {principal.status === "active" ? "無効化" : "有効化"}
                    </button>
                    <button
                      className="link"
                      onClick={() => {
                        if (window.confirm(`${principal.display_name} を削除しますか？作業履歴は残ります。`)) {
                          void run(() => api.delete(`/principals/${principal.id}`));
                        }
                      }}
                    >
                      削除
                    </button>
                  </>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {editing && <GrantEditor session={session} principal={editing} onClose={() => setEditing(null)} />}
      {renaming && (
        <RenamePrincipal
          principal={renaming}
          onClose={() => setRenaming(null)}
          onSaved={async () => {
            await reload();
            if (renaming.id === session.me.id) await session.reloadMe();
          }}
        />
      )}
    </div>
  );
}
