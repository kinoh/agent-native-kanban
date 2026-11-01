import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { navigate } from "../router";
import type { Project, Task } from "../types";

interface Item {
  id: string;
  label: string;
  hint: string;
  path: string;
}

export function Palette({ projects, onClose }: { projects: Project[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [tasks, setTasks] = useState<Task[]>([]);
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
  }, []);

  useEffect(() => {
    if (!query.trim()) {
      setTasks([]);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void api
        .get<{ tasks: Task[] }>(`/tasks?limit=8&sort=updated&q=${encodeURIComponent(query)}`)
        .then((reply) => !cancelled && setTasks(reply.tasks));
    }, 120);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  const items = useMemo<Item[]>(() => {
    const needle = query.toLowerCase();
    const matchingProjects = projects
      .filter((project) => project.name.toLowerCase().includes(needle) || project.key.toLowerCase().includes(needle))
      .map((project) => ({
        id: project.id,
        label: project.name,
        hint: "プロジェクト",
        path: `/projects/${project.key}/board`,
      }));
    const matchingTasks = tasks.map((task) => ({
      id: task.id,
      label: `${task.key} ${task.title}`,
      hint: task.stage.name,
      path: `/tasks/${task.key}`,
    }));
    return [...matchingTasks, ...matchingProjects];
  }, [projects, tasks, query]);

  const open = (item: Item | undefined) => {
    if (item) {
      navigate(item.path);
      onClose();
    }
  };

  return (
    <div className="modal-backdrop top" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-label="検索">
        <input
          ref={input}
          placeholder="タスクやプロジェクトを検索"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setIndex((value) => Math.min(value + 1, items.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setIndex((value) => Math.max(value - 1, 0));
            } else if (event.key === "Enter") {
              open(items[index]);
            } else if (event.key === "Escape") {
              onClose();
            }
          }}
        />
        <ul>
          {items.map((item, row) => (
            <li key={item.id} className={row === index ? "selected" : ""} onMouseDown={() => open(item)}>
              <span>{item.label}</span>
              <span className="muted">{item.hint}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
