import { type ReactNode, useEffect, useRef } from "react";
import { navigate } from "./router";
import type { BlockReason, Priority, PrincipalRef, Task, Work } from "./types";

export const PRIORITY_LABEL: Record<Priority, string> = {
  urgent: "緊急",
  high: "高",
  medium: "中",
  low: "低",
  none: "なし",
};

export const READINESS_LABEL: Record<Work["readiness"], string> = {
  ready: "着手可",
  waiting: "条件待ち",
  done: "作業完了",
  not_required: "作業なし",
  unknown: "状態不明",
};

export const ACTIVITY_LABEL: Record<NonNullable<Work["activity"]>, string> = {
  claimed: "取得済み",
  working: "作業中",
  paused: "一時停止",
};

const REASON_LABEL: Record<string, string> = {
  work_done: "この段階の作業は完了しています",
  no_work_required: "この段階には取得する作業がありません",
  on_hold: "保留中です",
  blocked_by_dependencies: "依存タスクが未完了です",
  occupied_not_started: "他の主体が取得しています",
  occupied_working: "他の主体が作業中です",
  occupied_paused: "占有者が一時停止中です",
  occupant_inactive: "占有者が無効になっています。管理者が解除してください",
  state_unknown: "記録された状態を解釈できないため、取得できません",
};

export function reasonText(reason: BlockReason, holdReason: string | null): string {
  const label = REASON_LABEL[reason.code] ?? reason.message;
  return reason.code === "on_hold" && holdReason ? `${label}: ${holdReason}` : label;
}

export function formatTime(value: string): string {
  return new Date(value).toLocaleString("ja-JP", { dateStyle: "short", timeStyle: "short" });
}

/** 履歴と管理画面で同じ委譲トークンを見分けるための短い表記。UUIDv7 は末尾が時刻に依らない。 */
export function shortTokenId(id: string): string {
  return id.slice(-8);
}

export function Avatar({ principal, size = 22 }: { principal: PrincipalRef; size?: number }) {
  return (
    <span
      className={`avatar ${principal.kind} ${principal.status === "active" ? "" : "inactive"}`}
      style={{ width: size, height: size, fontSize: size * 0.5 }}
      title={`${principal.display_name}（${principal.kind === "agent" ? "エージェント" : "人間"}）`}
    >
      {[...principal.display_name][0]?.toUpperCase()}
    </span>
  );
}

export function PrincipalLabel({ principal }: { principal: PrincipalRef | null }) {
  if (!principal) {
    return <span className="muted">未設定</span>;
  }
  return (
    <span className="principal">
      <Avatar principal={principal} />
      {principal.display_name}
      {principal.status !== "active" && <span className="muted">（{principal.status === "deleted" ? "削除済み" : "無効"}）</span>}
    </span>
  );
}

export function PriorityMark({ priority }: { priority: Priority }) {
  return (
    <span className={`priority priority-${priority}`} title={`優先度: ${PRIORITY_LABEL[priority]}`}>
      {priority === "urgent" ? "!" : "▮".repeat({ high: 3, medium: 2, low: 1, none: 0 }[priority])}
    </span>
  );
}

/** 業務段階とは別に、作業の状態だけを表す。 */
export function WorkBadge({ work }: { work: Work }) {
  const state = work.activity ?? work.readiness;
  const label = work.activity ? ACTIVITY_LABEL[work.activity] : READINESS_LABEL[work.readiness];
  return (
    <span className={`work-badge work-${state}`} title={work.reasons.map((r) => reasonText(r, work.hold_reason)).join("\n")}>
      <span className="dot" />
      {label}
    </span>
  );
}

export function TaskLink({ task }: { task: Pick<Task, "key" | "title"> }) {
  return (
    <a
      className="task-link"
      href={`/tasks/${task.key}`}
      onClick={(event) => {
        event.preventDefault();
        navigate(`/tasks/${task.key}`);
      }}
    >
      <span className="key">{task.key}</span> {task.title}
    </a>
  );
}

export function ErrorNote({ message }: { message: string | null }) {
  return message ? (
    <div className="error-note" role="alert">
      {message}
    </div>
  ) : null;
}

export function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    panel.current?.querySelector<HTMLElement>("input, textarea, select, button")?.focus();
  }, []);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
      onKeyDown={(event) => event.key === "Escape" && onClose()}
    >
      <div className="modal" role="dialog" aria-label={title} ref={panel}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

/** フォーム入力中の文字キーをショートカットとして扱わない判定。 */
export function isTyping(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return !!element && (["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName) || element.isContentEditable);
}
