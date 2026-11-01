import { type FormEvent, useState } from "react";
import { api } from "../api";
import type { Session } from "../App";
import { navigate } from "../router";
import { ErrorNote, Modal } from "../ui";

const KEY_RULE = "英大文字で始まる1〜10文字の英大文字・数字";

const DEFAULT_STAGES = [
  { name: "バックログ", requires_work: false, claim_permission: "claim", is_final: false },
  { name: "実装", requires_work: true, claim_permission: "claim", is_final: false },
  { name: "レビュー", requires_work: true, claim_permission: "review", is_final: false },
  { name: "完了", requires_work: false, claim_permission: "claim", is_final: true },
];

function CreateProject({ session, onClose }: { session: Session; onClose: () => void }) {
  const [key, setKey] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.post("/projects", { key, name, stages: DEFAULT_STAGES });
      await session.reloadProjects();
      onClose();
      navigate(`/projects/${key}/board`);
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  return (
    <Modal title="プロジェクトを作成" onClose={onClose}>
      <form onSubmit={submit}>
        <label>
          名前
          <input value={name} onChange={(e) => setName(e.target.value)} required />
        </label>
        <label>
          キー（FOO-123 の FOO の部分。{KEY_RULE}）
          <input
            value={key}
            onChange={(e) => {
              e.target.setCustomValidity("");
              setKey(e.target.value.toUpperCase());
            }}
            onInvalid={(e) => e.currentTarget.setCustomValidity(`キーは${KEY_RULE}にしてください`)}
            required
            pattern="[A-Z][A-Z0-9]{0,9}"
          />
        </label>
        <p className="muted">段階は作成後にプロジェクト設定で変更できます。</p>
        <ErrorNote message={error} />
        <button className="primary">作成</button>
      </form>
    </Modal>
  );
}

export function Projects({ session }: { session: Session }) {
  const [creating, setCreating] = useState(false);
  return (
    <div className="page">
      <header className="page-header">
        <h1>プロジェクト</h1>
        {session.me.is_org_admin && (
          <button className="primary" onClick={() => setCreating(true)}>
            プロジェクトを作成
          </button>
        )}
      </header>
      {session.projects.length === 0 && <p className="muted">閲覧できるプロジェクトはありません。</p>}
      <ul className="project-list">
        {session.projects.map((project) => (
          <li key={project.id}>
            <a
              href={`/projects/${project.key}/board`}
              onClick={(event) => {
                event.preventDefault();
                navigate(`/projects/${project.key}/board`);
              }}
            >
              <span className="key">{project.key}</span>
              {project.name}
            </a>
            <span className="muted">{project.stages.map((stage) => stage.name).join(" → ")}</span>
          </li>
        ))}
      </ul>
      {creating && <CreateProject session={session} onClose={() => setCreating(false)} />}
    </div>
  );
}
