import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { Session } from "../App";
import { navigate } from "../router";
import type { Project, Task } from "../types";
import { Avatar, DiscardBadge, ErrorNote, PriorityMark, WorkBadge, isTyping } from "../ui";
import { CreateTask, EMPTY_FILTERS, FilterBar, MoveStage, ProjectOverview, ViewTabs, canAdminProject, filterQuery, useProjectTasks } from "./common";

function CardBlockers({ task }: { task: Task }) {
  if (task.work.readiness !== "waiting" || task.work.blockers.length === 0) {
    return null;
  }
  return (
    <ul className="card-blockers" aria-label="依存先">
      {task.work.blockers.map((blocker) => (
        <li key={blocker.id} className="card-blocker">
          <a
            href={`/tasks/${blocker.key}`}
            onClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              navigate(`/tasks/${blocker.key}`);
            }}
          >
            {blocker.key}
          </a>
          <span>{blocker.reason}</span>
        </li>
      ))}
    </ul>
  );
}

function Card({
  task,
  selected,
  onSelect,
}: {
  task: Task;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <article
      className={["card", selected ? "selected" : "", task.discard ? "discarded" : ""].join(" ").trim()}
      data-task={task.key}
      draggable
      onDragStart={(event) => event.dataTransfer.setData("text/plain", task.key)}
      onClick={onSelect}
      onDoubleClick={() => navigate(`/tasks/${task.key}`)}
    >
      <div className="card-top">
        <span className="key">{task.key}</span>
        {task.discard && <DiscardBadge />}
        <PriorityMark priority={task.priority} />
      </div>
      <a
        className="card-title"
        href={`/tasks/${task.key}`}
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          navigate(`/tasks/${task.key}`);
        }}
      >
        {task.title}
      </a>
      <div className="card-work">
        <WorkBadge work={task.work} />
        {task.work.blockers.some((blocker) => blocker.discarded) && <span className="discarded-blocker">依存先が破棄</span>}
        {task.work.occupant && (
          <span className="occupant">
            <Avatar principal={task.work.occupant} size={18} />
            {task.work.occupant.display_name}
          </span>
        )}
      </div>
      <CardBlockers task={task} />
    </article>
  );
}

export function BoardView({ project, session }: { project: Project; session: Session }) {
  const [filters, setFilters] = useState(EMPTY_FILTERS);
  const { tasks, error, setError, reload } = useProjectTasks(project.id, filterQuery(filters));
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [moving, setMoving] = useState<Task | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);

  const columns = useMemo(
    () =>
      project.stages.map((stage) => ({
        stage,
        tasks: tasks.filter((task) => task.stage.id === stage.id),
      })),
    [project.stages, tasks],
  );

  const drop = async (key: string, stageId: string) => {
    const task = tasks.find((item) => item.key === key);
    if (!task || task.stage.id === stageId) {
      return;
    }
    try {
      await api.post(`/tasks/${key}/stage`, { to_stage_id: stageId, version: task.version });
      await reload();
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const focusedControl = event.target instanceof HTMLElement ? event.target.closest("a, button") : null;
      if (isTyping(event.target) || focusedControl || event.metaKey || event.ctrlKey || event.altKey || creating || moving) {
        return;
      }
      const column = columns.findIndex((item) => item.tasks.some((task) => task.key === selected));
      const row = column < 0 ? -1 : columns[column].tasks.findIndex((task) => task.key === selected);
      const firstFilled = columns.find((item) => item.tasks.length > 0);
      const moveTo = (next: number, nextRow: number) => {
        const target = columns[next];
        if (target?.tasks.length) {
          const clamped = Math.min(Math.max(nextRow, 0), target.tasks.length - 1);
          setSelected(target.tasks[clamped].key);
        }
      };
      if (["j", "k", "h", "l"].includes(event.key) && column < 0) {
        setSelected(firstFilled?.tasks[0].key ?? null);
      } else if (event.key === "j") {
        moveTo(column, row + 1);
      } else if (event.key === "k") {
        moveTo(column, row - 1);
      } else if (event.key === "h" || event.key === "l") {
        const step = event.key === "h" ? -1 : 1;
        let next = column + step;
        while (columns[next] && columns[next].tasks.length === 0) {
          next += step;
        }
        moveTo(next, row);
      } else if (event.key === "Enter" && selected) {
        navigate(`/tasks/${selected}`);
      } else if (event.key === "c") {
        event.preventDefault();
        setCreating(true);
      } else if (event.key === "m" && selected) {
        setMoving(tasks.find((task) => task.key === selected) ?? null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [columns, selected, tasks, creating, moving]);

  return (
    <div className="page board-page">
      <header className="page-header">
        <h1>{project.name}</h1>
        <ViewTabs project={project} current="board" />
        <button className="primary" onClick={() => setCreating(true)}>
          タスクを作成 <kbd>c</kbd>
        </button>
      </header>
      <ProjectOverview project={project} settingsLink={canAdminProject(session, project)} />
      <FilterBar filters={filters} onChange={setFilters} />
      <ErrorNote message={error} />
      <div className="board">
        {columns.map(({ stage, tasks: cards }) => (
          <section
            key={stage.id}
            className={dropTarget === stage.id ? "column drop-target" : "column"}
            data-stage={stage.name}
            onDragOver={(event) => {
              event.preventDefault();
              setDropTarget(stage.id);
            }}
            onDragLeave={() => setDropTarget(null)}
            onDrop={(event) => {
              event.preventDefault();
              setDropTarget(null);
              void drop(event.dataTransfer.getData("text/plain"), stage.id);
            }}
          >
            <h2>
              {stage.name} <span className="count">{cards.length}</span>
            </h2>
            {cards.map((task) => (
              <Card key={task.id} task={task} selected={selected === task.key} onSelect={() => setSelected(task.key)} />
            ))}
          </section>
        ))}
      </div>
      {creating && <CreateTask project={project} session={session} onClose={() => setCreating(false)} onCreated={() => void reload()} />}
      {moving && <MoveStage task={moving} stages={project.stages} onClose={() => setMoving(null)} onMoved={() => void reload()} />}
    </div>
  );
}
