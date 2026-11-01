import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useLiveChanges } from "../live";
import { navigate } from "../router";
import type { Session } from "../App";
import type { Principal, Project, Stage, Task } from "../types";
import { ErrorNote, Modal, PRIORITY_LABEL } from "../ui";

export function useProjectTasks(projectId: string, filters: string) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  const reload = useCallback(async () => {
    const current = ++requestId.current;
    try {
      const all: Task[] = [];
      let cursor = 0;
      while (true) {
        const reply = await api.get<{ tasks: Task[]; next_cursor: number | null }>(
          `/tasks?project=${projectId}&limit=500&cursor=${cursor}${filters}`,
        );
        if (current !== requestId.current) return;
        all.push(...reply.tasks);
        if (reply.next_cursor === null) break;
        if (!Number.isInteger(reply.next_cursor) || reply.next_cursor <= cursor) {
          throw new Error("タスク一覧の続きを取得できませんでした");
        }
        cursor = reply.next_cursor;
      }
      setTasks(all);
      setError(null);
    } catch (failure) {
      if (current !== requestId.current) return;
      console.error("Failed to load project tasks", failure);
      setError((failure as Error).message);
    }
  }, [projectId, filters]);

  useEffect(() => {
    setTasks([]);
    void reload();
    return () => { requestId.current += 1; };
  }, [reload]);
  useLiveChanges(() => void reload());
  return { tasks, error, setError, reload };
}

export function usePrincipals(): Principal[] {
  const [principals, setPrincipals] = useState<Principal[]>([]);
  useEffect(() => {
    void api.get<{ principals: Principal[] }>("/principals").then((reply) => setPrincipals(reply.principals));
  }, []);
  return principals.filter((principal) => principal.status !== "deleted");
}

export function canAdminProject(session: Session, project: Project) {
  return session.me.grants.some(
    (grant) => grant.permission === "admin" && (grant.project_id === null || grant.project_id === project.id),
  );
}

const OVERVIEW_COLLAPSED_LINES = 2;
const OVERVIEW_COLLAPSED_CHARS = 160;

export function ProjectOverview({ project, settingsLink }: { project: Project; settingsLink: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const text = project.description;
  if (!text) {
    if (!settingsLink) return null;
    return (
      <p className="project-overview-empty muted">
        <a
          href={`/projects/${project.key}/settings`}
          onClick={(event) => {
            event.preventDefault();
            navigate(`/projects/${project.key}/settings`);
          }}
        >
          プロジェクトの概要を設定
        </a>
      </p>
    );
  }
  const long = text.length > OVERVIEW_COLLAPSED_CHARS || text.split("\n").length > OVERVIEW_COLLAPSED_LINES;
  return (
    <section className="project-overview" aria-label="プロジェクトの概要">
      <p className={long && !expanded ? "collapsed" : ""}>{text}</p>
      {long && (
        <button type="button" className="link" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          {expanded ? "折りたたむ" : "すべて表示"}
        </button>
      )}
    </section>
  );
}

export function ViewTabs({ project, current }: { project: Project; current: "board" | "list" | "settings" }) {
  const tabs = [
    ["board", "ボード"],
    ["list", "リスト"],
    ["settings", "設定"],
  ] as const;
  return (
    <nav className="tabs">
      {tabs.map(([id, label]) => (
        <a
          key={id}
          className={id === current ? "active" : ""}
          href={`/projects/${project.key}/${id}`}
          onClick={(event) => {
            event.preventDefault();
            navigate(`/projects/${project.key}/${id}`);
          }}
        >
          {label}
        </a>
      ))}
    </nav>
  );
}

export interface Filters {
  query: string;
  claimableOnly: boolean;
  priority: string;
  dependsOn: string;
  sort: string;
}

export const EMPTY_FILTERS: Filters = { query: "", claimableOnly: false, priority: "", dependsOn: "", sort: "priority" };

export function filterQuery(filters: Filters): string {
  const params = new URLSearchParams();
  if (filters.query) params.set("q", filters.query);
  if (filters.claimableOnly) params.set("can_claim", "true");
  if (filters.priority) params.set("priority", filters.priority);
  if (filters.dependsOn.trim()) params.set("depends_on", filters.dependsOn.trim());
  params.set("sort", filters.sort);
  return `&${params}`;
}

export function FilterBar({ filters, onChange }: { filters: Filters; onChange: (filters: Filters) => void }) {
  return (
    <div className="filter-bar">
      <input
        className="filter-search"
        placeholder="タスクを絞り込む"
        value={filters.query}
        onChange={(e) => onChange({ ...filters, query: e.target.value })}
      />
      <label className="check">
        <input
          type="checkbox"
          checked={filters.claimableOnly}
          onChange={(e) => onChange({ ...filters, claimableOnly: e.target.checked })}
        />
        自分が取得できる仕事だけ
      </label>
      <input
        aria-label="依存先"
        placeholder="依存先のタスクキー"
        value={filters.dependsOn}
        onChange={(e) => onChange({ ...filters, dependsOn: e.target.value })}
      />
      <select value={filters.priority} onChange={(e) => onChange({ ...filters, priority: e.target.value })} aria-label="優先度">
        <option value="">優先度: すべて</option>
        {Object.entries(PRIORITY_LABEL).map(([value, label]) => (
          <option key={value} value={value}>
            {label}
          </option>
        ))}
      </select>
      <select value={filters.sort} onChange={(e) => onChange({ ...filters, sort: e.target.value })} aria-label="並べ替え">
        <option value="priority">優先度順</option>
        <option value="updated">更新順</option>
        <option value="created">作成順</option>
        <option value="due">期限順</option>
      </select>
    </div>
  );
}

export function CreateTask({
  project,
  stageId,
  onClose,
  onCreated,
}: {
  project: Project;
  stageId?: string;
  onClose: () => void;
  onCreated: () => void;
}) {
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [stage, setStage] = useState(stageId ?? project.stages[0]?.id ?? "");
  const [priority, setPriority] = useState("none");
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.post("/tasks", { project_id: project.id, title, description, stage_id: stage, priority });
      onCreated();
      onClose();
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  return (
    <Modal title="タスクを作成" onClose={onClose}>
      <form onSubmit={submit}>
        <label>
          タイトル
          <input value={title} onChange={(e) => setTitle(e.target.value)} required />
        </label>
        <label>
          説明
          <textarea rows={4} value={description} onChange={(e) => setDescription(e.target.value)} />
        </label>
        <div className="row">
          <label>
            業務段階
            <select value={stage} onChange={(e) => setStage(e.target.value)}>
              {project.stages.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            優先度
            <select value={priority} onChange={(e) => setPriority(e.target.value)}>
              {Object.entries(PRIORITY_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <ErrorNote message={error} />
        <button className="primary">作成</button>
      </form>
    </Modal>
  );
}

export function MoveStage({
  task,
  stages,
  onClose,
  onMoved,
}: {
  task: Task;
  stages: Stage[];
  onClose: () => void;
  onMoved: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const move = async (stage: Stage) => {
    try {
      await api.post(`/tasks/${task.key}/stage`, { to_stage_id: stage.id, version: task.version });
      onMoved();
      onClose();
    } catch (failure) {
      setError((failure as Error).message);
    }
  };
  return (
    <Modal title={`${task.key} の業務段階を変更`} onClose={onClose}>
      <div className="choice-list">
        {stages
          .filter((stage) => stage.id !== task.stage.id)
          .map((stage) => (
            <button key={stage.id} onClick={() => void move(stage)}>
              {stage.name}
            </button>
          ))}
      </div>
      <ErrorNote message={error} />
    </Modal>
  );
}
