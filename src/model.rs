//! 権限と取得可能性の判定。HTTP にも DB にも依存しない純粋な規則を置く。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    View,
    Create,
    Edit,
    Claim,
    Assign,
    Review,
    Approve,
    Admin,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Create => "create",
            Self::Edit => "edit",
            Self::Claim => "claim",
            Self::Assign => "assign",
            Self::Review => "review",
            Self::Approve => "approve",
            Self::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "view" => Self::View,
            "create" => Self::Create,
            "edit" => Self::Edit,
            "claim" => Self::Claim,
            "assign" => Self::Assign,
            "review" => Self::Review,
            "approve" => Self::Approve,
            "admin" => Self::Admin,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    /// 着手できる
    Ready,
    /// 依存または保留の解消を待っている
    Waiting,
    /// 現段階の作業が完了している
    Done,
    /// この段階には作業が定義されていない
    NotRequired,
    /// 記録された状態を解釈できない
    Unknown,
}

/// `Availability` の判定材料。DB の値をそのまま渡し、解釈できない値は判定の中で閉じる。
#[derive(Debug, Clone)]
pub struct WorkFacts {
    pub work_state: String,
    pub hold_reason: Option<String>,
    pub stage_requires_work: bool,
    pub occupant: Option<OccupantFacts>,
    /// 未完了の依存タスク数
    pub open_blockers: usize,
}

#[derive(Debug, Clone)]
pub struct OccupantFacts {
    pub activity: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockReason {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Availability {
    pub readiness: Readiness,
    pub claimable: bool,
    pub reasons: Vec<BlockReason>,
}

fn reason(code: &'static str, message: &str) -> BlockReason {
    BlockReason { code, message: message.to_string(), details: Value::Null }
}

/// 取得可能性の唯一の判定。業務段階の名称や担当者は材料に含めない。
/// 理由が1つでもあれば取得不可で、解釈できない値も理由に数える（不明なら取得可能としない）。
pub fn availability(facts: &WorkFacts) -> Availability {
    let mut reasons = Vec::new();

    let readiness = match (facts.work_state.as_str(), facts.stage_requires_work) {
        ("done", _) => {
            reasons.push(reason("work_done", "the work for this stage has been completed"));
            Readiness::Done
        }
        ("open" | "on_hold", false) => {
            reasons.push(reason("no_work_required", "this stage defines no work to claim"));
            Readiness::NotRequired
        }
        ("on_hold", true) => {
            let message = facts.hold_reason.as_deref().unwrap_or("no reason recorded");
            let mut hold = reason("on_hold", &format!("the work is on hold: {message}"));
            hold.details = json!({ "reason": facts.hold_reason });
            reasons.push(hold);
            Readiness::Waiting
        }
        ("open", true) if facts.open_blockers > 0 => Readiness::Waiting,
        ("open", true) => Readiness::Ready,
        (other, _) => {
            let mut unknown = reason("state_unknown", "the recorded work state is not recognized");
            unknown.details = json!({ "work_state": other });
            reasons.push(unknown);
            Readiness::Unknown
        }
    };

    if facts.open_blockers > 0 {
        let mut blocked = reason("blocked_by_dependencies", "dependent tasks are not finished");
        blocked.details = json!({ "open_blockers": facts.open_blockers });
        reasons.push(blocked);
    }

    if let Some(occupant) = &facts.occupant {
        reasons.push(match occupant.activity.as_str() {
            "claimed" => reason("occupied_not_started", "another principal holds this work"),
            "working" => reason("occupied_working", "another principal is working on this"),
            "paused" => reason("occupied_paused", "the holder has paused this work"),
            other => {
                let mut unknown =
                    reason("state_unknown", "the recorded work activity is not recognized");
                unknown.details = json!({ "activity": other });
                unknown
            }
        });
        // 無効化された主体の占有は自動で外さない。管理者の明示操作で解除する。
        if occupant.status != "active" {
            reasons.push(reason(
                "occupant_inactive",
                "the holder is no longer active; an administrator must release this work",
            ));
        }
    }

    Availability { readiness, claimable: reasons.is_empty(), reasons }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> WorkFacts {
        WorkFacts {
            work_state: "open".into(),
            hold_reason: None,
            stage_requires_work: true,
            occupant: None,
            open_blockers: 0,
        }
    }

    fn occupied(activity: &str) -> Option<OccupantFacts> {
        Some(OccupantFacts { activity: activity.into(), status: "active".into() })
    }

    fn codes(facts: &WorkFacts) -> Vec<&'static str> {
        availability(facts).reasons.iter().map(|r| r.code).collect()
    }

    #[test]
    fn ready_unoccupied_work_can_be_claimed() {
        let result = availability(&facts());
        assert!(result.claimable);
        assert_eq!(result.readiness, Readiness::Ready);
    }

    #[test]
    fn working_holder_blocks_others() {
        let f = WorkFacts { occupant: occupied("working"), ..facts() };
        assert!(!availability(&f).claimable);
        assert_eq!(codes(&f), ["occupied_working"]);
    }

    #[test]
    fn paused_holder_still_blocks_and_is_distinguishable() {
        let f = WorkFacts { occupant: occupied("paused"), ..facts() };
        assert!(!availability(&f).claimable);
        assert_eq!(codes(&f), ["occupied_paused"]);
    }

    #[test]
    fn unfinished_dependency_blocks_without_an_occupant() {
        let f = WorkFacts { open_blockers: 2, ..facts() };
        let result = availability(&f);
        assert!(!result.claimable);
        assert_eq!(result.readiness, Readiness::Waiting);
        assert_eq!(codes(&f), ["blocked_by_dependencies"]);
    }

    #[test]
    fn completed_work_cannot_be_claimed() {
        let f = WorkFacts { work_state: "done".into(), ..facts() };
        let result = availability(&f);
        assert!(!result.claimable);
        assert_eq!(result.readiness, Readiness::Done);
    }

    #[test]
    fn stage_without_work_cannot_be_claimed() {
        let f = WorkFacts { stage_requires_work: false, ..facts() };
        assert_eq!(codes(&f), ["no_work_required"]);
    }

    #[test]
    fn hold_reason_is_reported() {
        let f = WorkFacts {
            work_state: "on_hold".into(),
            hold_reason: Some("waiting for vendor".into()),
            ..facts()
        };
        let result = availability(&f);
        assert!(!result.claimable);
        assert!(result.reasons[0].message.contains("waiting for vendor"));
    }

    #[test]
    fn unknown_states_fail_closed() {
        let bad_state = WorkFacts { work_state: "corrupted".into(), ..facts() };
        assert_eq!(codes(&bad_state), ["state_unknown"]);
        let bad_activity = WorkFacts { occupant: occupied("sleeping"), ..facts() };
        assert_eq!(codes(&bad_activity), ["state_unknown"]);
    }

    #[test]
    fn inactive_holder_never_frees_the_work_implicitly() {
        let f = WorkFacts {
            occupant: Some(OccupantFacts { activity: "working".into(), status: "disabled".into() }),
            ..facts()
        };
        assert_eq!(codes(&f), ["occupied_working", "occupant_inactive"]);
    }
}
