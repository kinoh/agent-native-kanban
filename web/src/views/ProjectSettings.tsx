import { type FormEvent, useState } from "react";
import { api } from "../api";
import type { Session } from "../App";
import type { Project, Stage, TransitionRule } from "../types";
import { ErrorNote } from "../ui";
import { canAdminProject, ViewTabs } from "./common";

const CLAIM_PERMISSIONS = ["claim", "review", "approve"];
const RULE_PERMISSIONS = ["", "edit", "review", "approve", "admin"];

export function ProjectSettings({ project, session }: { project: Project; session: Session }) {
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [permission, setPermission] = useState("");
  const [requireDone, setRequireDone] = useState(false);
  // null は未編集。保存済みの値を表示し、他の主体の変更も反映する。
  const [overview, setOverview] = useState<string | null>(null);
  const [savingOverview, setSavingOverview] = useState(false);
  const [overviewSaved, setOverviewSaved] = useState(false);
  const canAdmin = canAdminProject(session, project);

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
      await session.reloadProjects();
      setError(null);
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  const saveOverview = async (event: FormEvent) => {
    event.preventDefault();
    setSavingOverview(true);
    setOverviewSaved(false);
    try {
      await api.patch(`/projects/${project.id}`, { description: overview ?? project.description });
      await session.reloadProjects();
      setOverview(null);
      setOverviewSaved(true);
      setError(null);
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setSavingOverview(false);
    }
  };

  const patchStage = (stage: Stage, fields: Partial<Stage>) =>
    run(() => api.patch(`/stages/${stage.id}`, fields));

  const swap = (index: number, offset: number) => {
    const here = project.stages[index];
    const there = project.stages[index + offset];
    return run(async () => {
      await api.patch(`/stages/${here.id}`, { position: there.position });
      await api.patch(`/stages/${there.id}`, { position: here.position });
    });
  };

  const addStage = (event: FormEvent) => {
    event.preventDefault();
    void run(() =>
      api.post(`/projects/${project.id}/stages`, {
        name,
        requires_work: true,
        claim_permission: "claim",
        is_final: false,
      }),
    ).then(() => setName(""));
  };

  const saveRules = (rules: Omit<TransitionRule, "id">[]) =>
    run(() =>
      api.put(`/projects/${project.id}/transition-rules`, {
        rules: rules.map(({ from_stage_id, to_stage_id, required_permission, require_work_done }) => ({
          from_stage_id,
          to_stage_id,
          required_permission,
          require_work_done,
        })),
      }),
    );

  const stageName = (id: string) => project.stages.find((stage) => stage.id === id)?.name;

  return (
    <div className="page">
      <header className="page-header">
        <h1>{project.name}</h1>
        <ViewTabs project={project} current="settings" />
      </header>
      <ErrorNote message={error} />
      {!canAdmin && <p className="muted">設定の変更にはこのプロジェクトの管理権限が必要です。</p>}
      <h2>概要</h2>
      {canAdmin ? (
        <form className="overview-form" onSubmit={saveOverview}>
          <textarea
            rows={5}
            aria-label="プロジェクトの概要"
            placeholder="プロジェクトの目的、リポジトリ名やURLなど"
            value={overview ?? project.description}
            onChange={(e) => {
              setOverview(e.target.value);
              setOverviewSaved(false);
            }}
          />
          <div>
            <button type="submit" disabled={savingOverview || overview === null}>
              {savingOverview ? "保存中…" : "概要を保存"}
            </button>
            {overviewSaved && <span className="muted"> 保存しました</span>}
          </div>
        </form>
      ) : (
        <p className="overview-text">{project.description || <span className="muted">概要は設定されていません。</span>}</p>
      )}
      <h2>業務段階</h2>
      <table className="settings-table">
        <thead>
          <tr>
            <th>順序</th>
            <th>名前</th>
            <th>作業あり</th>
            <th>取得に必要な権限</th>
            <th>最終段階</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {project.stages.map((stage, index) => (
            <tr key={stage.id}>
              <td>
                <button className="link" disabled={!canAdmin || index === 0} onClick={() => void swap(index, -1)}>↑</button>
                <button className="link" disabled={!canAdmin || index === project.stages.length - 1} onClick={() => void swap(index, 1)}>↓</button>
              </td>
              <td>
                <input
                  defaultValue={stage.name}
                  disabled={!canAdmin}
                  aria-label="段階名"
                  onBlur={(e) => e.target.value !== stage.name && void patchStage(stage, { name: e.target.value })}
                />
              </td>
              <td>
                <input type="checkbox" disabled={!canAdmin} checked={stage.requires_work} aria-label="作業あり" onChange={(e) => void patchStage(stage, { requires_work: e.target.checked })} />
              </td>
              <td>
                <select disabled={!canAdmin} value={stage.claim_permission} aria-label="取得権限" onChange={(e) => void patchStage(stage, { claim_permission: e.target.value })}>
                  {CLAIM_PERMISSIONS.map((value) => (
                    <option key={value}>{value}</option>
                  ))}
                </select>
              </td>
              <td>
                <input type="checkbox" disabled={!canAdmin} checked={stage.is_final} aria-label="最終段階" onChange={(e) => void patchStage(stage, { is_final: e.target.checked })} />
              </td>
              <td>
                <button className="link" disabled={!canAdmin} onClick={() => void run(() => api.delete(`/stages/${stage.id}`))}>削除</button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {canAdmin && (
        <form className="inline-form" onSubmit={addStage}>
          <input placeholder="新しい段階の名前" value={name} onChange={(e) => setName(e.target.value)} required />
          <button>段階を追加</button>
        </form>
      )}
      <h2>遷移ルール</h2>
      <p className="muted">ルールのない遷移は編集権限で実行できます。ルールがあると、指定の権限と条件が必要になります。</p>
      <ul className="related">
        {project.transition_rules.map((rule) => (
          <li key={rule.id}>
            {stageName(rule.from_stage_id)} → {stageName(rule.to_stage_id)}
            <span className="muted">
              {" "}権限: {rule.required_permission ?? "edit"}
              {rule.require_work_done ? "・作業完了が条件" : ""}
            </span>
            {canAdmin && (
              <button className="link" onClick={() => void saveRules(project.transition_rules.filter((item) => item.id !== rule.id))}>
                削除
              </button>
            )}
          </li>
        ))}
      </ul>
      {canAdmin && (
        <form
          className="inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            void saveRules([
              ...project.transition_rules,
              { from_stage_id: from, to_stage_id: to, required_permission: permission || null, require_work_done: requireDone },
            ]);
          }}
        >
          <select value={from} onChange={(e) => setFrom(e.target.value)} required aria-label="遷移元">
            <option value="">遷移元</option>
            {project.stages.map((stage) => <option key={stage.id} value={stage.id}>{stage.name}</option>)}
          </select>
          <select value={to} onChange={(e) => setTo(e.target.value)} required aria-label="遷移先">
            <option value="">遷移先</option>
            {project.stages.map((stage) => <option key={stage.id} value={stage.id}>{stage.name}</option>)}
          </select>
          <select value={permission} onChange={(e) => setPermission(e.target.value)} aria-label="必要な権限">
            {RULE_PERMISSIONS.map((value) => <option key={value} value={value}>{value || "権限: edit"}</option>)}
          </select>
          <label className="check">
            <input type="checkbox" checked={requireDone} onChange={(e) => setRequireDone(e.target.checked)} />
            作業完了を条件にする
          </label>
          <button>ルールを追加</button>
        </form>
      )}
    </div>
  );
}
