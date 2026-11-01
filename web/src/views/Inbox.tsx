import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { useLiveChanges } from "../live";
import type { InboxItem } from "../types";
import { ErrorNote, TaskLink, formatTime } from "../ui";

export function Inbox() {
  const [items, setItems] = useState<InboxItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    try {
      setItems((await api.get<{ requests: InboxItem[] }>("/me/inbox")).requests);
    } catch (failure) {
      setError((failure as Error).message);
    }
  }, []);
  useEffect(() => {
    void reload();
  }, [reload]);
  useLiveChanges(() => void reload());

  return (
    <div className="page">
      <header className="page-header">
        <h1>受信箱</h1>
      </header>
      <ErrorNote message={error} />
      {items.length === 0 && <p className="muted">自分宛の未回答の依頼はありません。</p>}
      <ul className="inbox">
        {items.map((item) => (
          <li key={item.id}>
            <TaskLink task={item.task} />
            <p>{item.body}</p>
            <span className="muted">
              {item.author.display_name} からの{item.kind === "question" ? "質問" : "レビュー依頼"}・{formatTime(item.created_at)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
