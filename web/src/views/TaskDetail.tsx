import { type FormEvent, useCallback, useEffect, useState } from "react";
import { api, uploadFile } from "../api";
import type { Session } from "../App";
import { useLiveChanges } from "../live";
import { navigate } from "../router";
import type { Principal, Project, Task, TaskContext } from "../types";
import {
  ACTIVITY_LABEL,
  ErrorNote,
  Modal,
  PRIORITY_LABEL,
  PrincipalLabel,
  READINESS_LABEL,
  TaskLink,
  WorkBadge,
  formatTime,
  isTyping,
} from "../ui";
import { MoveStage, usePrincipals } from "./common";

const COMMENT_KINDS: Record<string, string> = {
  comment: "コメント",
  progress: "進捗",
  decision: "判断",
  problem: "問題",
  question: "質問",
  review_request: "レビュー依頼",
  review_result: "レビュー結果",
  answer: "回答",
  handoff: "引継ぎ",
  completion: "完了報告",
};

const EVENT_LABELS: Record<string, string> = {
  "task.created": "タスクを作成",
  "task.updated": "タスクを編集",
  "stage.changed": "業務段階を変更",
  "work.claimed": "作業を取得",
  "work.started": "作業を開始",
  "work.paused": "作業を一時停止",
  "work.resumed": "作業を再開",
  "work.released": "作業を解放",
  "work.transferred": "作業を引継ぎ",
  "work.reassigned": "管理者が作業を移管",
  "work.revoked": "管理者が占有を解除",
  "work.completed": "作業完了を報告",
  "work.reopened": "作業を再開可能に戻した",
  "work.held": "作業を保留",
  "work.unheld": "保留を解除",
  "work.available": "取得可能になった",
  "work.unavailable": "取得不可になった",
  "comment.added": "コメントを記録",
  "artifact.added": "成果物を追加",
  "dependency.added": "依存関係を追加",
  "dependency.removed": "依存関係を削除",
  "link.added": "関連タスクを追加",
  "link.removed": "関連タスクを削除",
};

type Prompt =
  | { kind: "complete" | "release" | "hold" | "reopen" | "revoke" }
  | { kind: "transfer" | "reassign" };

const PROMPTS = {
  complete: { title: "作業完了を報告", label: "成果の要約", button: "完了を報告" },
  release: { title: "作業を解放", label: "残課題・引継ぎメモ（任意）", button: "解放" },
  hold: { title: "作業を保留", label: "保留の理由", button: "保留" },
  reopen: { title: "作業を再開可能に戻す", label: "理由", button: "再開可能にする" },
  revoke: { title: "占有を解除", label: "解除の理由", button: "解除" },
  transfer: { title: "作業を引継ぐ", label: "引継ぎメモ（成果・判断・残課題）", button: "引継ぐ" },
  reassign: { title: "作業を移管", label: "移管の理由", button: "移管" },
};

function WorkPanel({
  task,
  project,
  session,
  principals,
  onChanged,
  onError,
}: {
  task: Task;
  project: Project;
  session: Session;
  principals: Principal[];
  onChanged: () => void;
  onError: (message: string) => void;
}) {
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [text, setText] = useState("");
  const [target, setTarget] = useState("");
  const work = task.work;
  const path = `/tasks/${task.key}/work`;
  const claimRef = { claim_id: work.claim_id };
  const isAdmin = session.me.grants.some(
    (grant) => grant.permission === "admin" && (grant.project_id === null || grant.project_id === project.id),
  );
  const canEdit = isAdmin || session.me.grants.some(
    (grant) => grant.permission === "edit" && (grant.project_id === null || grant.project_id === project.id),
  );

  const run = async (operation: string, body: unknown = {}) => {
    try {
      await api.post(`${path}/${operation}`, body);
      onChanged();
      return true;
    } catch (failure) {
      onError((failure as Error).message);
      return false;
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!prompt) return;
    const body = {
      complete: { ...claimRef, summary: text },
      release: { ...claimRef, note: text.trim() ? text : undefined },
      hold: { reason: text },
      reopen: { reason: text },
      revoke: { reason: text },
      transfer: { ...claimRef, to: target, note: text },
      reassign: { to: target, reason: text },
    }[prompt.kind];
    if (await run(prompt.kind, body)) {
      setPrompt(null);
      setText("");
      setTarget("");
    }
  };

  const open = (kind: Prompt["kind"]) => {
    setText("");
    setPrompt({ kind } as Prompt);
  };

  return (
    <section className="work-panel">
      <h3>作業</h3>
      <div className="facts">
        <div>
          <span>作業準備</span>
          {READINESS_LABEL[work.readiness]}
        </div>
        <div>
          <span>作業者</span>
          <PrincipalLabel principal={work.occupant} />
        </div>
        <div>
          <span>活動</span>
          {work.activity ? ACTIVITY_LABEL[work.activity] : "—"}
        </div>
        <div>
          <span>取得</span>
          {work.can_claim
            ? "取得できます"
            : work.claimable
              ? `「${work.claim_permission}」権限が必要です`
              : "取得できません"}
        </div>
      </div>
      {work.reasons.length > 0 && (
        <ul className="reasons">
          {work.reasons.map((reason) => (
            <li key={reason.code}>{reason.message}</li>
          ))}
          {work.hidden_blockers > 0 && <li>閲覧権限のない依存タスクが {work.hidden_blockers} 件あります</li>}
        </ul>
      )}
      <div className="actions">
        {work.can_claim && (
          <button className="primary" onClick={() => void run("claim")}>
            取得する
          </button>
        )}
        {work.claim_id && work.activity === "claimed" && <button className="primary" onClick={() => void run("start", claimRef)}>開始</button>}
        {work.claim_id && work.activity === "working" && <button onClick={() => void run("pause", claimRef)}>一時停止</button>}
        {work.claim_id && work.activity === "paused" && <button className="primary" onClick={() => void run("resume", claimRef)}>再開</button>}
        {work.claim_id && <button onClick={() => open("complete")}>完了を報告</button>}
        {work.claim_id && <button onClick={() => open("transfer")}>引継ぐ</button>}
        {work.claim_id && <button onClick={() => open("release")}>解放</button>}
        {canEdit && work.readiness === "done" && <button onClick={() => open("reopen")}>再開可能に戻す</button>}
        {canEdit && !work.occupant && task.stage.requires_work && work.hold_reason === null && work.readiness !== "done" && (
          <button onClick={() => open("hold")}>保留</button>
        )}
        {canEdit && work.hold_reason !== null && <button onClick={() => void run("unhold")}>保留を解除</button>}
        {isAdmin && work.occupant && !work.claim_id && <button className="danger" onClick={() => open("revoke")}>占有を解除</button>}
        {isAdmin && work.occupant && !work.claim_id && <button className="danger" onClick={() => open("reassign")}>移管</button>}
      </div>
      {prompt && (
        <Modal title={PROMPTS[prompt.kind].title} onClose={() => setPrompt(null)}>
          <form onSubmit={submit}>
            {(prompt.kind === "transfer" || prompt.kind === "reassign") && (
              <label>
                引継ぎ先
                <select value={target} onChange={(e) => setTarget(e.target.value)} required>
                  <option value="">選択してください</option>
                  {principals
                    .filter((principal) => principal.status === "active" && principal.id !== work.occupant?.id)
                    .map((principal) => (
                      <option key={principal.id} value={principal.id}>
                        {principal.display_name}
                      </option>
                    ))}
                </select>
              </label>
            )}
            <label>
              {PROMPTS[prompt.kind].label}
              <textarea rows={5} value={text} onChange={(e) => setText(e.target.value)} required={prompt.kind !== "release"} />
            </label>
            <button className="primary">{PROMPTS[prompt.kind].button}</button>
          </form>
        </Modal>
      )}
    </section>
  );
}

function EditableText({
  label,
  value,
  onSave,
  large,
}: {
  label: string;
  value: string;
  onSave: (value: string) => Promise<void>;
  large?: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  if (!editing) {
    return (
      <div className="editable" onClick={() => {
        setDraft(value);
        setEditing(true);
      }}>
        <h3>{label}</h3>
        {value ? <p className={large ? "body-text" : ""}>{value}</p> : <p className="muted">クリックして入力</p>}
      </div>
    );
  }
  return (
    <div className="editable editing">
      <h3>{label}</h3>
      <textarea
        autoFocus
        rows={large ? 8 : 3}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(event) => event.key === "Escape" && setEditing(false)}
      />
      <div className="actions">
        <button
          className="primary"
          onClick={() => void onSave(draft).then(() => setEditing(false))}
        >
          保存
        </button>
        <button onClick={() => setEditing(false)}>取消</button>
      </div>
    </div>
  );
}

function CommentForm({
  taskKey,
  principals,
  parentId,
  onDone,
  onError,
}: {
  taskKey: string;
  principals: Principal[];
  parentId?: string;
  onDone: () => void;
  onError: (message: string) => void;
}) {
  const [kind, setKind] = useState(parentId ? "answer" : "comment");
  const [body, setBody] = useState("");
  const [addressee, setAddressee] = useState("");
  const needsAddressee = kind === "question" || kind === "review_request";

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.post(`/tasks/${taskKey}/comments`, {
        kind,
        body,
        addressee_id: needsAddressee ? addressee : undefined,
        parent_id: parentId,
      });
      setBody("");
      onDone();
    } catch (failure) {
      onError((failure as Error).message);
    }
  };

  return (
    <form className="comment-form" onSubmit={submit}>
      <textarea
        rows={parentId ? 2 : 3}
        placeholder={parentId ? "回答を入力" : "記録を残す"}
        value={body}
        onChange={(e) => setBody(e.target.value)}
        required
      />
      <div className="row">
        {!parentId && (
          <select value={kind} onChange={(e) => setKind(e.target.value)} aria-label="記録の種類">
            {["comment", "progress", "decision", "problem", "question", "review_request", "review_result"].map((value) => (
              <option key={value} value={value}>
                {COMMENT_KINDS[value]}
              </option>
            ))}
          </select>
        )}
        {needsAddressee && (
          <select value={addressee} onChange={(e) => setAddressee(e.target.value)} required aria-label="依頼先">
            <option value="">依頼先を選択</option>
            {principals
              .filter((principal) => principal.status === "active")
              .map((principal) => (
                <option key={principal.id} value={principal.id}>
                  {principal.display_name}
                </option>
              ))}
          </select>
        )}
        <button className="primary">{parentId ? "回答" : "記録"}</button>
      </div>
    </form>
  );
}

function historyDetail(type: string, data: Record<string, unknown>): string {
  if (type === "stage.changed") {
    const from = data.from as { name: string };
    const to = data.to as { name: string };
    return `${from.name} → ${to.name}`;
  }
  if (typeof data.reason === "string") return data.reason;
  if (type === "task.updated") return Object.keys((data.changes as object) ?? {}).join(", ");
  if (typeof data.depends_on === "string") return data.depends_on;
  if (typeof data.related === "string") return data.related;
  return "";
}

function RelationForm({
  taskKey,
  onDone,
  onError,
}: {
  taskKey: string;
  onDone: () => void;
  onError: (message: string) => void;
}) {
  const [type, setType] = useState("dependency");
  const [other, setOther] = useState("");
  const [reason, setReason] = useState("");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      if (type === "dependency") {
        await api.post(`/tasks/${taskKey}/dependencies`, { depends_on: other, reason });
      } else {
        await api.post(`/tasks/${taskKey}/links`, { related: other });
      }
      setOther("");
      setReason("");
      onDone();
    } catch (failure) {
      onError((failure as Error).message);
    }
  };
  return (
    <form className="relation-form" onSubmit={submit}>
      <select value={type} onChange={(e) => setType(e.target.value)} aria-label="関係の種類">
        <option value="dependency">依存先を追加</option>
        <option value="link">関連タスクを追加</option>
      </select>
      <input placeholder="タスクキー" value={other} onChange={(e) => setOther(e.target.value.toUpperCase())} required />
      {type === "dependency" && <input placeholder="理由" value={reason} onChange={(e) => setReason(e.target.value)} required />}
      <button>追加</button>
    </form>
  );
}

export function TaskDetail({ reference, session }: { reference: string; session: Session }) {
  const [context, setContext] = useState<TaskContext | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [moving, setMoving] = useState(false);
  const [historyFilter, setHistoryFilter] = useState("all");
  const principals = usePrincipals();

  const reload = useCallback(async () => {
    try {
      setContext(await api.get<TaskContext>(`/tasks/${reference}/context`));
      setLoadError(null);
    } catch (failure) {
      setLoadError((failure as Error).message);
    }
  }, [reference]);

  useEffect(() => {
    void reload();
  }, [reload]);
  useLiveChanges(() => void reload());

  const task = context?.task;
  const project = session.projects.find((item) => item.id === task?.project_id);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (isTyping(event.target) || event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === "m" && !moving) {
        setMoving(true);
      } else if (event.key === "Escape" && project && !moving && !document.querySelector(".modal")) {
        navigate(`/projects/${project.key}/board`);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [moving, project]);

  if (loadError) {
    return (
      <div className="page">
        <ErrorNote message={loadError} />
      </div>
    );
  }
  if (!context || !task || !project) {
    return null;
  }

  const update = async (fields: Record<string, unknown>) => {
    try {
      await api.patch(`/tasks/${task.key}`, { version: task.version, ...fields });
      setError(null);
      await reload();
    } catch (failure) {
      setError((failure as Error).message);
      await reload();
      throw failure;
    }
  };
  const quietUpdate = (fields: Record<string, unknown>) => void update(fields).catch(() => undefined);

  const onFile = async (file: File | undefined, role: string) => {
    if (!file) return;
    try {
      await uploadFile(task.key, file, role);
      await reload();
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  const comments = context.comments.filter((comment) => comment.parent_id === null);
  const answersTo = (id: string) => context.comments.filter((comment) => comment.parent_id === id);
  const history = context.history.filter(
    (event) => historyFilter === "all" || event.category === historyFilter,
  );

  return (
    <div className="page detail">
      <div className="detail-main">
        <div className="breadcrumbs">
          <a
            href={`/projects/${project.key}/board`}
            onClick={(event) => {
              event.preventDefault();
              navigate(`/projects/${project.key}/board`);
            }}
          >
            {project.name}
          </a>
          <span className="key">{task.key}</span>
        </div>
        <EditableTitle value={task.title} onSave={(title) => update({ title })} />
        <ErrorNote message={error} />
        <EditableText label="説明" large value={task.description} onSave={(description) => update({ description })} />
        <EditableText label="目的" value={task.purpose} onSave={(purpose) => update({ purpose })} />
        <EditableText label="完了条件" value={task.acceptance_criteria} onSave={(acceptance_criteria) => update({ acceptance_criteria })} />

        {context.parent && (
          <section>
            <h3>親タスク</h3>
            <TaskLink task={context.parent} />
          </section>
        )}
        {context.children.length > 0 && (
          <section>
            <h3>子タスク</h3>
            <ul className="related">
              {context.children.map((child) => (
                <li key={child.id}>
                  <TaskLink task={child} /> <WorkBadge work={child.work} />
                </li>
              ))}
            </ul>
          </section>
        )}
        <section>
          <h3>依存・関連</h3>
          <ul className="related">
            {context.dependencies.map(({ task: other, reason }) => (
              <li key={other.id}>
                <span className="muted">依存</span> <TaskLink task={other} /> <span className="muted">{reason}</span>
                <button className="link" onClick={() => void api.delete(`/tasks/${task.key}/dependencies/${other.key}`).then(reload, (f) => setError(f.message))}>
                  外す
                </button>
              </li>
            ))}
            {context.dependents.map((other) => (
              <li key={other.id}>
                <span className="muted">依存元</span> <TaskLink task={other} />
              </li>
            ))}
            {context.links.map((other) => (
              <li key={other.id}>
                <span className="muted">関連</span> <TaskLink task={other} />
                <button className="link" onClick={() => void api.delete(`/tasks/${task.key}/links/${other.key}`).then(reload, (f) => setError(f.message))}>
                  外す
                </button>
              </li>
            ))}
          </ul>
          <RelationForm taskKey={task.key} onDone={() => void reload()} onError={setError} />
        </section>

        <section>
          <h3>成果物・添付</h3>
          <ul className="related">
            {context.artifacts.map((artifact) => (
              <li key={artifact.id}>
                <span className="muted">{artifact.role === "artifact" ? "成果物" : "添付"}</span>{" "}
                <a href={artifact.url ?? `/api/v1/artifacts/${artifact.id}/content`} target="_blank" rel="noreferrer">
                  {artifact.name}
                </a>
              </li>
            ))}
          </ul>
          <label className="file-button">
            ファイルを添付
            <input type="file" hidden onChange={(e) => void onFile(e.target.files?.[0], "attachment")} />
          </label>
        </section>

        <section>
          <h3>記録</h3>
          <ul className="comments">
            {comments.map((comment) => (
              <li key={comment.id} className={`comment kind-${comment.kind}`}>
                <div className="comment-head">
                  <strong>{comment.author.display_name}</strong>
                  <span className="kind-tag">{COMMENT_KINDS[comment.kind]}</span>
                  {comment.addressee_id && (
                    <span className="muted">→ {principals.find((p) => p.id === comment.addressee_id)?.display_name}</span>
                  )}
                  <span className="muted">{formatTime(comment.created_at)}</span>
                </div>
                <p>{comment.body}</p>
                {answersTo(comment.id).map((answer) => (
                  <div className="answer" key={answer.id}>
                    <strong>{answer.author.display_name}</strong> {answer.body}
                  </div>
                ))}
                {(comment.kind === "question" || comment.kind === "review_request") && !comment.resolved_at && (
                  <CommentForm taskKey={task.key} principals={principals} parentId={comment.id} onDone={() => void reload()} onError={setError} />
                )}
              </li>
            ))}
          </ul>
          <CommentForm taskKey={task.key} principals={principals} onDone={() => void reload()} onError={setError} />
        </section>

        <section>
          <div className="section-head">
            <h3>履歴</h3>
            <select value={historyFilter} onChange={(e) => setHistoryFilter(e.target.value)} aria-label="履歴の種類">
              <option value="all">すべて</option>
              <option value="stage">業務段階</option>
              <option value="work">作業状態</option>
            </select>
          </div>
          <ul className="history">
            {history.map((event) => (
              <li key={event.seq} className={`history-${event.category}`}>
                <span className="muted">{formatTime(event.created_at)}</span>{" "}
                {event.actor?.display_name ?? "システム"} が {EVENT_LABELS[event.type] ?? event.type}
                <span className="muted"> {historyDetail(event.type, event.data)}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>

      <aside className="detail-side">
        <section>
          <h3>業務段階</h3>
          <button className="stage-button" onClick={() => setMoving(true)}>
            {task.stage.name} <kbd>m</kbd>
          </button>
        </section>
        <WorkPanel task={task} project={project} session={session} principals={principals} onChanged={() => void reload()} onError={setError} />
        <section className="fields">
          <label>
            責任者
            <select value={task.responsible?.id ?? ""} onChange={(e) => quietUpdate({ responsible_id: e.target.value || null })}>
              <option value="">未設定</option>
              {principals.map((principal) => (
                <option key={principal.id} value={principal.id}>
                  {principal.display_name}
                </option>
              ))}
            </select>
          </label>
          <label>
            担当者
            <select value={task.assignee?.id ?? ""} onChange={(e) => quietUpdate({ assignee_id: e.target.value || null })}>
              <option value="">未設定</option>
              {principals.map((principal) => (
                <option key={principal.id} value={principal.id}>
                  {principal.display_name}
                </option>
              ))}
            </select>
          </label>
          <label>
            優先度
            <select value={task.priority} onChange={(e) => quietUpdate({ priority: e.target.value })}>
              {Object.entries(PRIORITY_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            期限
            <input
              type="date"
              value={task.due_at?.slice(0, 10) ?? ""}
              onChange={(e) => quietUpdate({ due_at: e.target.value ? `${e.target.value}T00:00:00Z` : null })}
            />
          </label>
        </section>
      </aside>
      {moving && <MoveStage task={task} stages={project.stages} onClose={() => setMoving(false)} onMoved={() => void reload()} />}
    </div>
  );
}

function EditableTitle({ value, onSave }: { value: string; onSave: (value: string) => Promise<void> }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      className="title-input"
      aria-label="タイトル"
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        if (draft.trim() && draft !== value) {
          onSave(draft).catch(() => setDraft(value));
        }
      }}
      onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
    />
  );
}
