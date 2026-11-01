import { type FormEvent, useEffect, useState } from "react";
import { ApiFailure, api } from "../api";
import type { Session } from "../App";
import { ErrorNote } from "../ui";

export function AccountSettings({ session }: { session: Session }) {
  const [displayName, setDisplayName] = useState(session.me.display_name);
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [profileError, setProfileError] = useState<string | null>(null);
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [profileSaved, setProfileSaved] = useState(false);
  const [passwordSaved, setPasswordSaved] = useState(false);
  const [savingProfile, setSavingProfile] = useState(false);
  const [savingPassword, setSavingPassword] = useState(false);

  useEffect(() => setDisplayName(session.me.display_name), [session.me.display_name]);

  const saveProfile = async (event: FormEvent) => {
    event.preventDefault();
    const name = displayName.trim();
    if (!name) {
      setProfileError("表示名を入力してください");
      return;
    }
    setSavingProfile(true);
    setProfileError(null);
    setProfileSaved(false);
    try {
      await api.patch("/me", { display_name: name });
      await session.reloadMe();
      setProfileSaved(true);
    } catch (failure) {
      setProfileError((failure as Error).message);
    } finally {
      setSavingProfile(false);
    }
  };

  const savePassword = async (event: FormEvent) => {
    event.preventDefault();
    if (newPassword !== confirmation) {
      setPasswordError("新しいパスワードが一致しません");
      return;
    }
    setSavingPassword(true);
    setPasswordError(null);
    setPasswordSaved(false);
    try {
      await api.put("/me/password", { current_password: currentPassword, new_password: newPassword });
      setCurrentPassword("");
      setNewPassword("");
      setConfirmation("");
      setPasswordSaved(true);
    } catch (failure) {
      setPasswordError(failure instanceof ApiFailure && failure.status === 401 ? "現在のパスワードが違います" : (failure as Error).message);
    } finally {
      setSavingPassword(false);
    }
  };

  return (
    <div className="page account-settings">
      <header className="page-header"><h1>アカウント設定</h1></header>
      <section>
        <h2>表示名</h2>
        <form className="settings-form" onSubmit={(event) => void saveProfile(event)}>
          <label>
            表示名
            <input value={displayName} maxLength={100} onChange={(event) => setDisplayName(event.target.value)} required />
          </label>
          <div className="actions"><button className="primary" disabled={savingProfile}>{savingProfile ? "保存中" : "表示名を保存"}</button></div>
        </form>
        {profileSaved && <p className="success-note" role="status">保存しました</p>}
        <ErrorNote message={profileError} />
      </section>
      <section>
        <h2>パスワード</h2>
        <form className="settings-form" onSubmit={(event) => void savePassword(event)}>
          <label>
            現在のパスワード
            <input type="password" autoComplete="current-password" value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} required />
          </label>
          <label>
            新しいパスワード
            <input type="password" autoComplete="new-password" minLength={12} value={newPassword} onChange={(event) => setNewPassword(event.target.value)} required />
          </label>
          <label>
            新しいパスワードを確認
            <input type="password" autoComplete="new-password" minLength={12} value={confirmation} onChange={(event) => setConfirmation(event.target.value)} required />
          </label>
          <div className="actions"><button className="primary" disabled={savingPassword}>{savingPassword ? "変更中" : "パスワードを変更"}</button></div>
        </form>
        {passwordSaved && <p className="success-note" role="status">変更しました</p>}
        <ErrorNote message={passwordError} />
      </section>
    </div>
  );
}
