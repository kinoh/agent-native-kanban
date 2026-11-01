import { type FormEvent, useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Session } from "../App";
import type { Resource, ResourceHold } from "../types";
import { ErrorNote, Modal, PrincipalLabel, formatTime } from "../ui";
import { useLiveChanges } from "../live";

function scopeLabel(resource: Resource, session: Session): string {
  if (!resource.project_id) return "組織";
  return session.projects.find((project) => project.id === resource.project_id)?.name ?? "プロジェクト";
}

function canAdmin(resource: Resource, session: Session): boolean {
  return session.me.is_org_admin || (!!resource.project_id && session.me.grants.some(
    (grant) => grant.project_id === resource.project_id && grant.permission === "admin",
  ));
}

function canAcquire(resource: Resource, session: Session): boolean {
  if (session.me.is_org_admin) return true;
  return session.me.grants.some((grant) =>
    (grant.permission === "claim" || grant.permission === "admin")
    && (!resource.project_id || grant.project_id === null || grant.project_id === resource.project_id),
  );
}

function ResourceEditor({ resource, onClose, onSaved }: { resource?: Resource; onClose: () => void; onSaved: () => Promise<void> }) {
  const [name, setName] = useState(resource?.name ?? "");
  const [description, setDescription] = useState(resource?.description ?? "");
  const [capacity, setCapacity] = useState(resource?.capacity ?? 1);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const save = async (event: FormEvent) => {
    event.preventDefault(); setSaving(true); setError(null);
    try {
      await api.patch(`/resources/${resource!.id}`, { name, description, capacity });
      await onSaved(); onClose();
    } catch (failure) { setError((failure as Error).message); } finally { setSaving(false); }
  };
  return (
    <Modal title="リソースを編集" onClose={onClose}>
      <form onSubmit={(event) => void save(event)}>
        <label>名前<input value={name} maxLength={100} onChange={(event) => setName(event.target.value)} required /></label>
        <label>説明<textarea value={description} onChange={(event) => setDescription(event.target.value)} /></label>
        <label>上限<input type="number" min={1} value={capacity} onChange={(event) => setCapacity(Number(event.target.value))} required /></label>
        <div className="actions"><button className="primary" disabled={saving}>{saving ? "保存中" : "変更を保存"}</button><button type="button" onClick={onClose} disabled={saving}>キャンセル</button></div>
        <ErrorNote message={error} />
      </form>
    </Modal>
  );
}

function HoldRow({ hold, resource, session, reload, report }: { hold: ResourceHold; resource: Resource; session: Session; reload: () => Promise<void>; report: (error: unknown) => void }) {
  const [reason, setReason] = useState("");
  const [revoking, setRevoking] = useState(false);
  const mine = hold.holder.id === session.me.id;
  const admin = canAdmin(resource, session);
  const release = async () => { try { await api.post(`/resources/${resource.id}/release`, { hold_id: hold.id }); await reload(); } catch (error) { report(error); } };
  const revoke = async (event: FormEvent) => { event.preventDefault(); try { await api.post(`/resources/${resource.id}/revoke`, { hold_id: hold.id, reason }); setRevoking(false); await reload(); } catch (error) { report(error); } };
  return <li className="resource-hold">
    <PrincipalLabel principal={hold.holder} />
    <span>{hold.reason}</span>
    <time>{formatTime(hold.acquired_at)}</time>
    {mine && <button className="link" onClick={() => void release()}>解放</button>}
    {admin && !revoking && <button className="link" onClick={() => setRevoking(true)}>解除</button>}
    {revoking && <form className="inline-form" onSubmit={(event) => void revoke(event)}><input aria-label="解除理由" placeholder="解除理由" value={reason} maxLength={500} onChange={(event) => setReason(event.target.value)} required /><button className="danger">解除する</button><button type="button" onClick={() => setRevoking(false)}>キャンセル</button></form>}
  </li>;
}

function ResourceCard({ resource, session, reload, report, edit }: { resource: Resource; session: Session; reload: () => Promise<void>; report: (error: unknown) => void; edit: () => void }) {
  const [reason, setReason] = useState("");
  const [acquiring, setAcquiring] = useState(false);
  const acquire = async (event: FormEvent) => { event.preventDefault(); setAcquiring(true); try { await api.post(`/resources/${resource.id}/acquire`, { reason }); setReason(""); await reload(); } catch (error) { report(error); } finally { setAcquiring(false); } };
  const remove = async () => { if (!window.confirm(`「${resource.name}」を削除する？`)) return; try { await api.delete(`/resources/${resource.id}`); await reload(); } catch (error) { report(error); } };
  const admin = canAdmin(resource, session);
  return <article className="resource-card">
    <header><div><h2>{resource.name}</h2><span className="muted">{scopeLabel(resource, session)}</span></div><strong>{resource.holds.length} / {resource.capacity}</strong></header>
    {resource.description && <p>{resource.description}</p>}
    <ul className="resource-holds">
      {resource.holds.map((hold) => <HoldRow key={hold.id} hold={hold} resource={resource} session={session} reload={reload} report={report} />)}
    </ul>
    {canAcquire(resource, session) && <form className="inline-form" onSubmit={(event) => void acquire(event)}><input aria-label={`${resource.name}を確保する理由`} placeholder="確保理由" value={reason} maxLength={500} onChange={(event) => setReason(event.target.value)} required /><button className="primary" disabled={acquiring}>{acquiring ? "確保中" : "確保"}</button></form>}
    {admin && <div className="actions"><button onClick={edit}>編集</button><button className="danger" onClick={() => void remove()}>削除</button></div>}
  </article>;
}

export function Resources({ session }: { session: Session }) {
  const [resources, setResources] = useState<Resource[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<Resource | null>(null);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [capacity, setCapacity] = useState(1);
  const [projectId, setProjectId] = useState("");
  const reload = useCallback(async () => {
    try { const reply = await api.get<{ resources: Resource[] }>("/resources"); setResources(reply.resources); setError(null); }
    catch (failure) { setError((failure as Error).message); throw failure; }
  }, []);
  useEffect(() => { void reload().catch(() => undefined); }, [reload]);
  useLiveChanges(() => { void reload().catch(() => undefined); });
  const create = async (event: FormEvent) => { event.preventDefault(); try { await api.post("/resources", { project_id: projectId || undefined, name, description, capacity }); setName(""); setDescription(""); setCapacity(1); setProjectId(""); setAdding(false); await reload(); } catch (failure) { setError((failure as Error).message); } };
  const creatableProjects = session.projects.filter((project) => session.me.is_org_admin || session.me.grants.some((grant) => grant.project_id === project.id && grant.permission === "admin"));
  const canCreate = session.me.is_org_admin || creatableProjects.length > 0;
  return <div className="page resources-page">
    <header className="page-header"><h1>リソース</h1>{canCreate && <button className="primary" onClick={() => setAdding(true)}>リソースを追加</button>}</header>
    <ErrorNote message={error} />
    <div className="resource-grid">{resources.map((resource) => <ResourceCard key={resource.id} resource={resource} session={session} reload={reload} report={(failure) => setError((failure as Error).message)} edit={() => setEditing(resource)} />)}</div>
    {adding && <Modal title="リソースを追加" onClose={() => setAdding(false)}><form onSubmit={(event) => void create(event)}>
      <label>所属<select value={projectId} onChange={(event) => setProjectId(event.target.value)}><option value="">組織</option>{creatableProjects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}</select></label>
      <label>名前<input value={name} maxLength={100} onChange={(event) => setName(event.target.value)} required /></label>
      <label>説明<textarea value={description} onChange={(event) => setDescription(event.target.value)} /></label>
      <label>上限<input type="number" min={1} value={capacity} onChange={(event) => setCapacity(Number(event.target.value))} required /></label>
      <div className="actions"><button className="primary">リソースを追加</button><button type="button" onClick={() => setAdding(false)}>キャンセル</button></div>
    </form></Modal>}
    {editing && <ResourceEditor resource={editing} onClose={() => setEditing(null)} onSaved={reload} />}
  </div>;
}
