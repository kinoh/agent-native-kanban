import { useCallback, useEffect, useRef, useState } from "react";
import { ApiFailure, api } from "./api";
import { navigate, usePath } from "./router";
import type { InboxItem, Me, Project } from "./types";
import { isTyping } from "./ui";
import { Admin } from "./views/Admin";
import { AccountSettings } from "./views/AccountSettings";
import { BoardView } from "./views/Board";
import { Inbox } from "./views/Inbox";
import { ListView } from "./views/List";
import { Login } from "./views/Login";
import { Palette } from "./views/Palette";
import { Projects } from "./views/Projects";
import { ProjectSettings } from "./views/ProjectSettings";
import { Resources } from "./views/Resources";
import { Signup } from "./views/Signup";
import { TaskDetail } from "./views/TaskDetail";
import { useLiveChanges } from "./live";

export interface Session {
  me: Me;
  projects: Project[];
  reloadProjects: () => Promise<void>;
  reloadMe: () => Promise<void>;
}

function Help({ onClose }: { onClose: () => void }) {
  const keys: [string, string][] = [
    ["⌘K / Ctrl+K", "検索・移動"],
    ["c", "タスクを作成"],
    ["j / k", "カードを移動"],
    ["h / l", "列を移動（ボード）"],
    ["Enter", "選択したタスクを開く"],
    ["m", "業務段階を変更"],
    ["g b", "ボードへ"],
    ["g l", "リストへ"],
    ["g i", "受信箱へ"],
    ["Esc", "閉じる・戻る"],
    ["?", "このヘルプ"],
  ];
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal">
        <h2>キーボード操作</h2>
        <dl className="shortcuts">
          {keys.map(([key, label]) => (
            <div key={key}>
              <dt>{key}</dt>
              <dd>{label}</dd>
            </div>
          ))}
        </dl>
      </div>
    </div>
  );
}

function NavLink({ to, label, current, badge }: { to: string; label: string; current: string; badge?: number }) {
  return (
    <a
      className={current === to || current.startsWith(`${to}/`) ? "nav-link active" : "nav-link"}
      href={to}
      onClick={(event) => {
        event.preventDefault();
        navigate(to);
      }}
    >
      {label}
      {!!badge && <span className="badge">{badge}</span>}
    </a>
  );
}

function Shell({ me, onLogout, reloadMe }: { me: Me; onLogout: () => void; reloadMe: () => Promise<void> }) {
  const path = usePath();
  const [projects, setProjects] = useState<Project[]>([]);
  const [inbox, setInbox] = useState(0);
  const [palette, setPalette] = useState(false);
  const [help, setHelp] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const projectsRequest = useRef(0);

  const reloadProjects = useCallback(async () => {
    const current = ++projectsRequest.current;
    try {
      const reply = await api.get<{ projects: Project[] }>("/projects");
      if (current !== projectsRequest.current) return;
      setProjects(reply.projects);
      setLoadError(null);
    } catch (error) {
      if (current !== projectsRequest.current) return;
      console.error("Failed to load projects", error);
      setLoadError((error as Error).message);
    }
  }, []);
  const reloadInbox = useCallback(async () => {
    const reply = await api.get<{ requests: InboxItem[] }>("/me/inbox");
    setInbox(reply.requests.length);
  }, []);

  useEffect(() => {
    void reloadProjects();
    void reloadInbox().catch((error) => setLoadError(error.message));
  }, [reloadProjects, reloadInbox]);
  useLiveChanges(() => {
    void reloadProjects();
    void reloadInbox().catch((error) => setLoadError(error.message));
  });

  const projectMatch = path.match(/^\/projects\/([^/]+)/);
  const currentProject = projects.find((project) => project.key === projectMatch?.[1]);

  useEffect(() => {
    let pendingG = false;
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPalette(true);
        return;
      }
      if (isTyping(event.target) || event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      if (pendingG) {
        pendingG = false;
        const base = currentProject ? `/projects/${currentProject.key}` : null;
        if (event.key === "b" && base) navigate(`${base}/board`);
        if (event.key === "l" && base) navigate(`${base}/list`);
        if (event.key === "i") navigate("/inbox");
        return;
      }
      if (event.key === "g") {
        pendingG = true;
      } else if (event.key === "?") {
        setHelp(true);
      } else if (event.key === "Escape") {
        setHelp(false);
        setPalette(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [currentProject]);

  const session: Session = { me, projects, reloadProjects, reloadMe };
  const taskMatch = path.match(/^\/tasks\/([^/]+)/);
  let content;
  if (taskMatch) {
    content = <TaskDetail key={taskMatch[1]} reference={taskMatch[1]} session={session} />;
  } else if (path === "/inbox") {
    content = <Inbox />;
  } else if (path === "/resources") {
    content = <Resources session={session} />;
  } else if (path === "/admin" && me.is_org_admin) {
    content = <Admin session={session} />;
  } else if (path === "/settings" && me.kind === "human") {
    content = <AccountSettings session={session} />;
  } else if (currentProject && path.endsWith("/board")) {
    content = <BoardView project={currentProject} />;
  } else if (currentProject && path.endsWith("/list")) {
    content = <ListView project={currentProject} />;
  } else if (currentProject && path.endsWith("/settings")) {
    content = <ProjectSettings project={currentProject} session={session} />;
  } else {
    content = <Projects session={session} />;
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="org">{me.organization.name}</div>
        <button className="search-button" onClick={() => setPalette(true)}>
          検索 <kbd>⌘K</kbd>
        </button>
        <NavLink to="/inbox" label="受信箱" current={path} badge={inbox} />
        <NavLink to="/projects" label="プロジェクト" current={path === "/" ? "/projects" : path} />
        <NavLink to="/resources" label="リソース" current={path} />
        <div className="nav-group">
          {projects.map((project) => (
            <NavLink key={project.id} to={`/projects/${project.key}/board`} label={project.name} current={path} />
          ))}
        </div>
        {me.is_org_admin && <NavLink to="/admin" label="組織の管理" current={path} />}
        <div className="spacer" />
        <div className="me">
          {me.kind === "human" ? <NavLink to="/settings" label={me.display_name} current={path} /> : <span>{me.display_name}</span>}
          <button className="link" onClick={onLogout}>
            ログアウト
          </button>
        </div>
      </aside>
      <main className="content">
        {loadError && <div className="error-note">{loadError}</div>}
        {content}
      </main>
      {palette && <Palette projects={projects} onClose={() => setPalette(false)} />}
      {help && <Help onClose={() => setHelp(false)} />}
    </div>
  );
}

export function App() {
  const path = usePath();
  const [me, setMe] = useState<Me | null | undefined>(undefined);
  const [fatal, setFatal] = useState<string | null>(null);

  const reloadInitialSession = useCallback(async () => {
    try {
      setMe(await api.get<Me>("/me"));
    } catch (error) {
      if (error instanceof ApiFailure && error.status === 401) {
        setMe(null);
      } else {
        setFatal((error as Error).message);
        throw error;
      }
    }
  }, []);

  const refreshMe = useCallback(async () => {
    try {
      setMe(await api.get<Me>("/me"));
    } catch (error) {
      if (error instanceof ApiFailure && error.status === 401) {
        setMe(null);
      }
      throw error;
    }
  }, []);

  useEffect(() => {
    void reloadInitialSession().catch(() => undefined);
  }, [reloadInitialSession]);

  const signup = path.match(/^\/signup\/([^/]+)$/);
  if (signup) {
    return <Signup token={signup[1]} onDone={() => void reloadInitialSession().then(() => navigate("/")).catch(() => undefined)} />;
  }
  if (fatal) {
    return <div className="center-card error-note">{fatal}</div>;
  }
  if (me === undefined) {
    return null;
  }
  if (me === null) {
    return <Login onDone={() => void reloadInitialSession().catch(() => undefined)} />;
  }
  return (
    <Shell
      me={me}
      reloadMe={refreshMe}
      onLogout={() =>
        void api.post("/auth/logout").then(() => {
          setMe(null);
          navigate("/");
        })
      }
    />
  );
}
