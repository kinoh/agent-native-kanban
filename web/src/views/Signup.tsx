import { type FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import { ErrorNote, formatTime } from "../ui";

interface LinkInfo {
  organization: string;
  note: string;
  expires_at: string;
}

export function Signup({ token, onDone }: { token: string; onDone: () => void }) {
  const [info, setInfo] = useState<LinkInfo | null>(null);
  const [linkError, setLinkError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  useEffect(() => {
    api.get<LinkInfo>(`/signup/${token}`).then(setInfo, (failure) => setLinkError(failure.message));
  }, [token]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.post(`/signup/${token}`, { display_name: name, email, password });
      onDone();
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  if (linkError) {
    return (
      <div className="center-card">
        <h1>登録できません</h1>
        <ErrorNote message={linkError} />
      </div>
    );
  }
  if (!info) {
    return null;
  }
  return (
    <form className="center-card" onSubmit={submit}>
      <h1>{info.organization} に参加</h1>
      <p className="muted">このリンクの有効期限: {formatTime(info.expires_at)}</p>
      <label>
        表示名
        <input value={name} onChange={(e) => setName(e.target.value)} required autoFocus />
      </label>
      <label>
        メールアドレス
        <input type="email" autoComplete="username" value={email} onChange={(e) => setEmail(e.target.value)} required />
      </label>
      <label>
        パスワード（12文字以上）
        <input type="password" autoComplete="new-password" minLength={12} value={password} onChange={(e) => setPassword(e.target.value)} required />
      </label>
      <ErrorNote message={error} />
      <button className="primary">登録する</button>
    </form>
  );
}
