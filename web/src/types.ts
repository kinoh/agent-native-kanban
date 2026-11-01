export type Permission = "view" | "create" | "edit" | "claim" | "assign" | "review" | "approve" | "admin";

export interface PrincipalRef {
  id: string;
  kind: "human" | "agent";
  display_name: string;
  status: string;
}

export interface Principal extends PrincipalRef {
  description: string;
  created_at: string;
  email?: string | null;
}

export interface PrincipalWorkItem {
  key: string;
  title: string;
  project: Pick<Project, "id" | "key" | "name">;
  activity: "claimed" | "working" | "paused";
  acquired_at: string;
}

export interface PrincipalWork extends Principal {
  work: PrincipalWorkItem[];
}

export interface Me {
  id: string;
  kind: string;
  display_name: string;
  email: string | null;
  organization: { id: string; name: string };
  is_org_admin: boolean;
  grants: { project_id: string | null; permission: Permission }[];
}

export interface Stage {
  id: string;
  name: string;
  position: number;
  requires_work: boolean;
  claim_permission: string;
  is_final: boolean;
}

export interface TransitionRule {
  id: string;
  from_stage_id: string;
  to_stage_id: string;
  required_permission: string | null;
  require_work_done: boolean;
}

export interface Project {
  id: string;
  key: string;
  name: string;
  description: string;
  stages: Stage[];
  transition_rules: TransitionRule[];
}

export interface BlockReason {
  code: string;
  message: string;
}

export interface Work {
  readiness: "ready" | "waiting" | "done" | "not_required" | "unknown";
  claimable: boolean;
  can_claim: boolean;
  claim_permission: string;
  has_claim_permission: boolean;
  reasons: BlockReason[];
  occupant: PrincipalRef | null;
  activity: "claimed" | "working" | "paused" | null;
  is_working: boolean;
  acquired_at: string | null;
  claim_id: string | null;
  hold_reason: string | null;
  blockers: { id: string; key: string; reason: string }[];
  hidden_blockers: number;
}

export interface Task {
  id: string;
  key: string;
  project_id: string;
  title: string;
  description: string;
  purpose: string;
  acceptance_criteria: string;
  stage: Stage;
  responsible: PrincipalRef | null;
  assignee: PrincipalRef | null;
  priority: Priority;
  due_at: string | null;
  parent_id: string | null;
  version: number;
  created_at: string;
  updated_at: string;
  work: Work;
}

export type Priority = "urgent" | "high" | "medium" | "low" | "none";

export interface Comment {
  id: string;
  kind: string;
  body: string;
  addressee_id: string | null;
  parent_id: string | null;
  resolved_at: string | null;
  created_at: string;
  author: { id: string; display_name: string; kind: string };
}

export interface Artifact {
  id: string;
  role: string;
  name: string;
  url: string | null;
  size: number | null;
  created_at: string;
}

export interface HistoryEvent {
  seq: number;
  type: string;
  category: string;
  actor: { id: string; display_name: string; kind: string } | null;
  delegated_token_id: string | null;
  data: Record<string, unknown>;
  created_at: string;
}

export interface TaskContext {
  task: Task;
  parent: Task | null;
  children: Task[];
  dependencies: { reason: string | null; task: Task }[];
  dependents: Task[];
  links: Task[];
  comments: Comment[];
  artifacts: Artifact[];
  history: HistoryEvent[];
}

export interface InboxItem {
  id: string;
  kind: string;
  body: string;
  created_at: string;
  task: { id: string; key: string; title: string };
  author: { id: string; display_name: string; kind: string };
}

export interface ApiToken {
  id: string;
  name: string;
  created_at: string;
  expires_at: string | null;
  revoked_at: string | null;
  parent_token_id: string | null;
  /** 委譲トークンだけが持つ。 */
  scopes?: { project_id: string; permission: Permission }[];
  tasks?: string[];
  task_keys?: string[];
}

export interface SignupLink {
  id: string;
  note: string;
  max_uses: number;
  used_count: number;
  expires_at: string;
  revoked_at: string | null;
}

export interface ResourceHold {
  id: string;
  reason: string;
  acquired_at: string;
  holder: PrincipalRef;
}

export interface Resource {
  id: string;
  org_id: string;
  project_id: string | null;
  name: string;
  description: string;
  capacity: number;
  created_at: string;
  updated_at: string;
  holds: ResourceHold[];
}
