import { type FormEvent, useState } from "react";
import { api } from "../api";
import { ErrorNote } from "../ui";

export function Login({ onDone }: { onDone: () => void }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await api.post("/auth/login", { email, password });
      onDone();
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  return (
    <form className="center-card" onSubmit={submit}>
      <h1>ログイン</h1>
      <label>
        メールアドレス
        <input type="email" autoComplete="username" value={email} onChange={(e) => setEmail(e.target.value)} required autoFocus />
      </label>
      <label>
        パスワード
        <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
      </label>
      <ErrorNote message={error} />
      <button className="primary">ログイン</button>
    </form>
  );
}
