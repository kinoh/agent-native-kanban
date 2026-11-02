import { useEffect, useState } from "react";
import type { Session } from "../App";
import { navigate } from "../router";
import type { Project, Task } from "../types";
import { DiscardBadge, ErrorNote, PrincipalLabel, PriorityMark, WorkBadge, formatTime, isTyping } from "../ui";
import { CreateTask, EMPTY_FILTERS, FilterBar, MoveStage, ProjectOverview, ViewTabs, canAdminProject, filterQuery, useProjectTasks } from "./common";

export function ListView({ project, session }: { project: Project; session: Session }) {
  const [filters, setFilters] = useState(EMPTY_FILTERS);
  const { tasks, error, reload } = useProjectTasks(project.id, filterQuery(filters));
  const [index, setIndex] = useState(0);
  const [creating, setCreating] = useState(false);
  const [moving, setMoving] = useState<Task | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (isTyping(event.target) || event.metaKey || event.ctrlKey || event.altKey || creating || moving) {
        return;
      }
      if (event.key === "j") {
        setIndex((value) => Math.min(value + 1, tasks.length - 1));
      } else if (event.key === "k") {
        setIndex((value) => Math.max(value - 1, 0));
      } else if (event.key === "Enter" && tasks[index]) {
        navigate(`/tasks/${tasks[index].key}`);
      } else if (event.key === "c") {
        event.preventDefault();
        setCreating(true);
      } else if (event.key === "m" && tasks[index]) {
        setMoving(tasks[index]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [tasks, index, creating, moving]);

  return (
    <div className="page">
      <header className="page-header">
        <h1>{project.name}</h1>
        <ViewTabs project={project} current="list" />
        <button className="primary" onClick={() => setCreating(true)}>
          タスクを作成 <kbd>c</kbd>
        </button>
      </header>
      <ProjectOverview project={project} settingsLink={canAdminProject(session, project)} />
      <FilterBar filters={filters} onChange={setFilters} />
      <ErrorNote message={error} />
      <table className="task-table">
        <thead>
          <tr>
            <th>キー</th>
            <th>タイトル</th>
            <th>優先度</th>
            <th>業務段階</th>
            <th>作業状態</th>
            <th>作業者</th>
            <th>更新</th>
          </tr>
        </thead>
        <tbody>
          {tasks.map((task, row) => (
            <tr key={task.id} className={[row === index ? "selected" : "", task.discard ? "discarded" : ""].join(" ").trim()} onClick={() => navigate(`/tasks/${task.key}`)}>
              <td className="key">{task.key}</td>
              <td>
                <span className="task-title">{task.title}</span> {task.discard && <DiscardBadge />}
              </td>
              <td>
                <PriorityMark priority={task.priority} />
              </td>
              <td>{task.stage.name}</td>
              <td>
                <WorkBadge work={task.work} />
              </td>
              <td>{task.work.occupant ? <PrincipalLabel principal={task.work.occupant} /> : <span className="muted">—</span>}</td>
              <td className="muted">{formatTime(task.updated_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {tasks.length === 0 && <p className="muted">該当するタスクはありません。</p>}
      {creating && <CreateTask project={project} onClose={() => setCreating(false)} onCreated={() => void reload()} />}
      {moving && <MoveStage task={moving} stages={project.stages} onClose={() => setMoving(null)} onMoved={() => void reload()} />}
    </div>
  );
}
