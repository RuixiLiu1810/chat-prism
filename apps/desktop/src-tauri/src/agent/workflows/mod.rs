mod paper_drafting;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use paper_drafting::PaperDraftingStage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkflowType {
    PaperDrafting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowCheckpointDecision {
    ApproveStage,
    RequestChanges,
}

impl WorkflowCheckpointDecision {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "approve" | "approve_stage" => Some(Self::ApproveStage),
            "reject" | "request_changes" | "request_change" => Some(Self::RequestChanges),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStageRecord {
    pub stage: String,
    pub prompt_summary: Option<String>,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkflowState {
    pub workflow_id: String,
    pub workflow_type: AgentWorkflowType,
    pub tab_id: String,
    pub local_session_id: Option<String>,
    pub project_path: String,
    pub model: Option<String>,
    pub current_stage: PaperDraftingStage,
    pub pending_checkpoint: bool,
    pub stage_history: Vec<WorkflowStageRecord>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCheckpointTransition {
    pub workflow_type: AgentWorkflowType,
    pub from_stage: String,
    pub to_stage: String,
    pub completed: bool,
}

impl AgentWorkflowState {
    pub fn new_paper_drafting(tab_id: &str, project_path: &str, model: Option<String>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            workflow_id: Uuid::new_v4().to_string(),
            workflow_type: AgentWorkflowType::PaperDrafting,
            tab_id: tab_id.to_string(),
            local_session_id: None,
            project_path: project_path.to_string(),
            model,
            current_stage: PaperDraftingStage::OutlineConfirmation,
            pending_checkpoint: false,
            stage_history: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn stage_label(&self) -> &'static str {
        self.current_stage.label()
    }

    pub fn is_completed(&self) -> bool {
        self.current_stage.is_terminal()
    }

    pub fn can_run_stage(&self) -> Result<(), String> {
        if self.pending_checkpoint {
            return Err(format!(
                "Workflow checkpoint is pending at stage '{}'. Approve or request changes before continuing.",
                self.current_stage.as_str()
            ));
        }
        if self.is_completed() {
            return Err("Workflow already completed. Start a new workflow to continue drafting.".to_string());
        }
        Ok(())
    }

    pub fn mark_stage_completed(&mut self, prompt: &str) {
        self.pending_checkpoint = true;
        self.stage_history.push(WorkflowStageRecord {
            stage: self.current_stage.as_str().to_string(),
            prompt_summary: summarize_prompt(prompt),
            completed_at: Utc::now().to_rfc3339(),
        });
        self.updated_at = Utc::now().to_rfc3339();
    }

    pub fn apply_checkpoint_decision(
        &mut self,
        decision: WorkflowCheckpointDecision,
    ) -> Result<WorkflowCheckpointTransition, String> {
        if !self.pending_checkpoint {
            return Err("No pending workflow checkpoint to resolve.".to_string());
        }

        let from = self.current_stage.as_str().to_string();
        match decision {
            WorkflowCheckpointDecision::ApproveStage => {
                self.pending_checkpoint = false;
                if let Some(next_stage) = self.current_stage.next_stage() {
                    self.current_stage = next_stage;
                } else {
                    self.current_stage = PaperDraftingStage::Completed;
                }
                self.updated_at = Utc::now().to_rfc3339();
                Ok(WorkflowCheckpointTransition {
                    workflow_type: self.workflow_type.clone(),
                    from_stage: from,
                    to_stage: self.current_stage.as_str().to_string(),
                    completed: self.current_stage.is_terminal(),
                })
            }
            WorkflowCheckpointDecision::RequestChanges => {
                self.pending_checkpoint = false;
                self.updated_at = Utc::now().to_rfc3339();
                Ok(WorkflowCheckpointTransition {
                    workflow_type: self.workflow_type.clone(),
                    from_stage: from.clone(),
                    to_stage: from,
                    completed: false,
                })
            }
        }
    }

    pub fn bind_local_session_id(&mut self, local_session_id: Option<&str>) {
        if let Some(local_session_id) = local_session_id.filter(|value| !value.trim().is_empty()) {
            self.local_session_id = Some(local_session_id.to_string());
            self.updated_at = Utc::now().to_rfc3339();
        }
    }

    pub fn build_stage_prompt(&self, user_prompt: &str) -> String {
        let mut lines = vec![
            "[Workflow mode: paper_drafting]".to_string(),
            format!("[Workflow stage: {}]", self.current_stage.as_str()),
            format!("[Stage objective: {}]", self.current_stage.instruction()),
            "[Checkpoint rule: complete only this stage in this turn; do not silently advance to the next stage.]".to_string(),
        ];

        if !self.stage_history.is_empty() {
            lines.push("[Completed stages so far:]".to_string());
            for entry in self.stage_history.iter().rev().take(3).rev() {
                let suffix = entry
                    .prompt_summary
                    .as_deref()
                    .map(|summary| format!(" — {}", summary))
                    .unwrap_or_default();
                lines.push(format!("- {}{}", entry.stage, suffix));
            }
        }

        format!("{}\n\n{}", lines.join("\n"), user_prompt)
    }
}

fn summarize_prompt(prompt: &str) -> Option<String> {
    prompt
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('['))
        .map(|line| {
            if line.chars().count() > 160 {
                format!("{}...", line.chars().take(160).collect::<String>())
            } else {
                line.to_string()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{AgentWorkflowState, WorkflowCheckpointDecision};

    #[test]
    fn paper_drafting_workflow_advances_only_after_checkpoint_approval() {
        let mut workflow =
            AgentWorkflowState::new_paper_drafting("tab-1", "/tmp/project", None);

        assert!(workflow.can_run_stage().is_ok());
        workflow.mark_stage_completed("Drafted the outline.");
        assert!(workflow.can_run_stage().is_err());

        let transition = workflow
            .apply_checkpoint_decision(WorkflowCheckpointDecision::ApproveStage)
            .unwrap();
        assert_eq!(transition.from_stage, "outline_confirmation");
        assert_eq!(transition.to_stage, "section_drafting");
        assert!(!transition.completed);
        assert!(workflow.can_run_stage().is_ok());
    }

    #[test]
    fn paper_drafting_workflow_reject_keeps_stage() {
        let mut workflow =
            AgentWorkflowState::new_paper_drafting("tab-1", "/tmp/project", None);

        workflow.mark_stage_completed("Outline draft.");
        let transition = workflow
            .apply_checkpoint_decision(WorkflowCheckpointDecision::RequestChanges)
            .unwrap();
        assert_eq!(transition.from_stage, "outline_confirmation");
        assert_eq!(transition.to_stage, "outline_confirmation");
        assert!(!transition.completed);
        assert!(workflow.can_run_stage().is_ok());
    }
}
