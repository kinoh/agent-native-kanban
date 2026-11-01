import { type FormEvent, useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Session } from "../App";
import type { Permission, Principal, SignupLink } from "../types";
import { Avatar, ErrorNote, Modal, formatTime } from "../ui";

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
  const [project, setProject] = useState("");
  const [permission, setPermission] = useState<Permission>("view");
  const [token, setToken] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const path = `/principals/${principal.id}`;

  const reload = useCallback(async () => {
    const reply = await api.get<{ grants: Grant[] }>(`${path}/grants`);
    setGrants(reply.grants);
  }, [path]);
  useEffect(() => {
    void reload();
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
      <ErrorNote message={error} />
    </Modal>
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
    }
  }, []);
  useEffect(() => {
    void reload();
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
    </div>
  );
}
