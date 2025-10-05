use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::sync::Mutex;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters, ServerHandler},
    model::{ErrorData as McpError, *},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json;

/// Deliberate thinking request parameters
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeliberateThinkingRequest {
    #[schemars(description = "Current thinking step")]
    pub thought: String,
    #[serde(rename = "nextThoughtNeeded")]
    #[schemars(description = "Whether another thought step is needed")]
    pub next_thought_needed: bool,
    #[serde(rename = "thoughtNumber")]
    #[schemars(description = "Current thought number (minimum 1)", range(min = 1))]
    pub thought_number: u32,
    #[serde(rename = "totalThoughts")]
    #[schemars(
        description = "Estimated total thoughts needed (minimum 1)",
        range(min = 1)
    )]
    pub total_thoughts: u32,
    #[serde(rename = "isRevision", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether this revises previous thinking")]
    pub is_revision: Option<bool>,
    #[serde(rename = "revisesThought", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Which thought number is being reconsidered")]
    pub revises_thought: Option<u32>,
    #[serde(rename = "branchFromThought", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Branching point thought number")]
    pub branch_from_thought: Option<u32>,
    #[serde(rename = "branchId", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Branch identifier")]
    pub branch_id: Option<String>,
    #[serde(rename = "needsMoreThoughts", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "If more thoughts are needed")]
    pub needs_more_thoughts: Option<bool>,
    #[serde(rename = "role", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Team role submitting this update")]
    pub role: Option<TeamRole>,
    #[serde(
        rename = "discussionPoints",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    #[schemars(description = "Key discussion points raised during this iteration")]
    pub discussion_points: Vec<DiscussionPoint>,
    #[serde(
        rename = "backlogStories",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    #[schemars(description = "Backlog stories to add or update")]
    pub backlog_stories: Vec<BacklogItem>,
    #[serde(
        rename = "removeStoryIds",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    #[schemars(description = "Backlog story identifiers slated for removal")]
    pub remove_story_ids: Vec<String>,
    #[serde(rename = "sprintPlan", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Sprint plan proposal from the project manager")]
    pub sprint_plan: Option<SprintPlan>,
    #[serde(rename = "consensusUpdate", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Consensus status update for the iteration")]
    pub consensus_update: Option<ConsensusUpdate>,
    #[serde(rename = "requiresUserInput", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether the team needs user input before proceeding")]
    pub requires_user_input: Option<bool>,
}

impl DeliberateThinkingRequest {
    /// Validates the request parameters
    fn validate(&self) -> Result<(), McpError> {
        validate_min_value("thoughtNumber", self.thought_number, 1)?;
        validate_min_value("totalThoughts", self.total_thoughts, 1)?;

        if let Some(revises) = self.revises_thought {
            validate_min_value("revisesThought", revises, 1)?;
        }

        if let Some(branch_from) = self.branch_from_thought {
            validate_min_value("branchFromThought", branch_from, 1)?;
        }

        if let Some(role) = &self.role {
            if matches!(role, TeamRole::ProjectManager) && self.thought.trim().is_empty() {
                return Err(create_validation_error(
                    "Project manager updates must include a summary thought",
                ));
            }
        }

        for point in &self.discussion_points {
            validate_non_empty("discussionPoints.detail", &point.detail)?;
        }

        for story in &self.backlog_stories {
            validate_non_empty("backlogStories.id", &story.id)?;
            validate_non_empty("backlogStories.title", &story.title)?;
        }

        for story_id in &self.remove_story_ids {
            validate_non_empty("removeStoryIds[]", story_id)?;
        }

        if let Some(plan) = &self.sprint_plan {
            validate_non_empty("sprintPlan.sprintName", &plan.sprint_name)?;
            validate_non_empty("sprintPlan.goal", &plan.goal)?;
            validate_min_value("sprintPlan.durationDays", plan.duration_days, 1)?;
        }

        Ok(())
    }
}

/// Response for deliberate thinking tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliberateThinkingResponse {
    #[serde(rename = "thoughtNumber")]
    pub thought_number: u32,
    #[serde(rename = "totalThoughts")]
    pub total_thoughts: u32,
    #[serde(rename = "nextThoughtNeeded")]
    pub next_thought_needed: bool,
    pub branches: Vec<String>,
    #[serde(rename = "thoughtHistoryLength")]
    pub thought_history_length: u32,
    #[serde(rename = "pmReport")]
    pub pm_report: ProjectManagerReport,
}

impl DeliberateThinkingResponse {
    /// Creates a new response from a request and state info
    fn new(
        request: &DeliberateThinkingRequest,
        branches: Vec<String>,
        thought_history_length: u32,
        pm_report: ProjectManagerReport,
    ) -> Self {
        Self {
            thought_number: request.thought_number,
            total_thoughts: request.total_thoughts,
            next_thought_needed: request.next_thought_needed,
            branches,
            pm_report,
            thought_history_length,
        }
    }
}

/// Internal thought data for tracking
#[derive(Debug, Clone)]
pub struct ThoughtData {
    pub thought: String,
    pub thought_number: u32,
    pub total_thoughts: u32,
    pub next_thought_needed: bool,
    pub is_revision: Option<bool>,
    pub revises_thought: Option<u32>,
    pub branch_from_thought: Option<u32>,
    pub branch_id: Option<String>,
    pub needs_more_thoughts: Option<bool>,
}

impl From<DeliberateThinkingRequest> for ThoughtData {
    fn from(req: DeliberateThinkingRequest) -> Self {
        Self {
            thought: req.thought,
            thought_number: req.thought_number,
            total_thoughts: req.total_thoughts,
            next_thought_needed: req.next_thought_needed,
            is_revision: req.is_revision,
            revises_thought: req.revises_thought,
            branch_from_thought: req.branch_from_thought,
            branch_id: req.branch_id,
            needs_more_thoughts: req.needs_more_thoughts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum TeamRole {
    ProjectManager,
    PragmaticProgrammer,
    ProductVisionary,
}

impl fmt::Display for TeamRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TeamRole::ProjectManager => write!(f, "Project Manager"),
            TeamRole::PragmaticProgrammer => write!(f, "Pragmatic Programmer"),
            TeamRole::ProductVisionary => write!(f, "Product Visionary"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscussionPoint {
    #[schemars(description = "Team role that raised this point")]
    pub role: TeamRole,
    #[schemars(description = "Summary of the discussion item")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PriorityLevel {
    High,
    Medium,
    Low,
}

impl PriorityLevel {
    fn rank(&self) -> u8 {
        match self {
            PriorityLevel::High => 0,
            PriorityLevel::Medium => 1,
            PriorityLevel::Low => 2,
        }
    }
}

impl fmt::Display for PriorityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PriorityLevel::High => write!(f, "High"),
            PriorityLevel::Medium => write!(f, "Medium"),
            PriorityLevel::Low => write!(f, "Low"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StoryStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
}

impl StoryStatus {
    fn rank(&self) -> u8 {
        match self {
            StoryStatus::InProgress => 0,
            StoryStatus::Todo => 1,
            StoryStatus::Blocked => 2,
            StoryStatus::Done => 3,
        }
    }
}

impl fmt::Display for StoryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoryStatus::Todo => write!(f, "To Do"),
            StoryStatus::InProgress => write!(f, "In Progress"),
            StoryStatus::Blocked => write!(f, "Blocked"),
            StoryStatus::Done => write!(f, "Done"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BacklogItem {
    #[schemars(description = "Unique identifier for the story")]
    pub id: String,
    #[schemars(description = "User story title or summary")]
    pub title: String,
    #[schemars(description = "Priority for the story")]
    pub priority: PriorityLevel,
    #[schemars(description = "Current delivery status")]
    pub status: StoryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Team role currently accountable for the story")]
    pub owner: Option<TeamRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Additional implementation notes")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SprintParticipant {
    #[schemars(description = "Role assigned to this sprint")]
    pub role: TeamRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Reason this role is included in the sprint")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Key responsibilities for this role")]
    pub responsibilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SprintPlan {
    #[schemars(description = "Sprint name or identifier")]
    pub sprint_name: String,
    #[schemars(description = "Goal for the sprint")]
    pub goal: String,
    #[schemars(description = "Planned duration of the sprint in days", range(min = 1))]
    pub duration_days: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Participants for the sprint with rationale")]
    pub participants: Vec<SprintParticipant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Backlog stories committed to this sprint")]
    pub committed_story_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Known risks tracked by the project manager")]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsensusUpdate {
    #[schemars(description = "Whether the team agrees code changes are ready")]
    pub ready_for_code_changes: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(description = "Outstanding blockers identified by the team")]
    pub blockers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Additional notes from the project manager")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsensusState {
    pub ready_for_code_changes: bool,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Default for ConsensusState {
    fn default() -> Self {
        Self {
            ready_for_code_changes: false,
            blockers: Vec::new(),
            notes: None,
        }
    }
}

#[derive(Debug, Clone)]
struct BacklogChange {
    change_type: BacklogChangeType,
    item: BacklogItem,
}

impl BacklogChange {
    fn new(change_type: BacklogChangeType, item: BacklogItem) -> Self {
        Self { change_type, item }
    }

    fn summary(&self) -> String {
        match self.change_type {
            BacklogChangeType::Added => format!(
                "Added {} [{} | {}]",
                self.item.id, self.item.priority, self.item.status
            ),
            BacklogChangeType::Updated => format!(
                "Updated {} -> {} [{}]",
                self.item.id, self.item.status, self.item.priority
            ),
            BacklogChangeType::Removed => {
                format!("Removed {} ({})", self.item.id, self.item.title)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum BacklogChangeType {
    Added,
    Updated,
    Removed,
}

#[derive(Debug, Default)]
struct TeamUpdateOutcome {
    pm_summary: Option<String>,
    new_discussion_points: Vec<DiscussionPoint>,
    backlog_changes: Vec<BacklogChange>,
    sprint_plan_updated: Option<SprintPlan>,
    consensus_state: Option<ConsensusState>,
    awaiting_user_input: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManagerReport {
    #[schemars(description = "Structured bullet points shared with the user")]
    pub bullets: Vec<String>,
    #[serde(rename = "pmSummary", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Latest project manager summary narrative")]
    pub pm_summary: Option<String>,
    #[serde(
        rename = "newDiscussionPoints",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    #[schemars(description = "Discussion points captured during this iteration")]
    pub new_discussion_points: Vec<DiscussionPoint>,
    #[serde(
        rename = "backlogSnapshot",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    #[schemars(description = "Current backlog ordered by priority and status")]
    pub backlog_snapshot: Vec<BacklogItem>,
    #[serde(rename = "activeSprint", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Active sprint plan the team is executing")]
    pub active_sprint: Option<SprintPlan>,
    #[serde(rename = "consensus")]
    #[schemars(description = "Latest consensus state for moving forward")]
    pub consensus: ConsensusState,
    #[serde(rename = "waitingOnUser")]
    #[schemars(description = "Whether the team is awaiting input from the user")]
    pub waiting_on_user: bool,
}

#[derive(Debug, Clone)]
pub struct TeamState {
    pm_summaries: Vec<String>,
    discussion_log: Vec<DiscussionPoint>,
    backlog: HashMap<String, BacklogItem>,
    active_sprint: Option<SprintPlan>,
    consensus: ConsensusState,
    awaiting_user_input: bool,
}

impl Default for TeamState {
    fn default() -> Self {
        Self {
            pm_summaries: Vec::new(),
            discussion_log: Vec::new(),
            backlog: HashMap::new(),
            active_sprint: None,
            consensus: ConsensusState::default(),
            awaiting_user_input: false,
        }
    }
}

impl TeamState {
    fn process_request(&mut self, request: &DeliberateThinkingRequest) -> TeamUpdateOutcome {
        let mut outcome = TeamUpdateOutcome::default();

        if !request.discussion_points.is_empty() {
            for point in &request.discussion_points {
                self.discussion_log.push(point.clone());
            }
            outcome
                .new_discussion_points
                .extend(request.discussion_points.clone());
        }

        if let Some(role) = &request.role {
            if matches!(role, TeamRole::ProjectManager) {
                let summary = request.thought.trim();
                if !summary.is_empty() {
                    let summary = summary.to_string();
                    self.pm_summaries.push(summary.clone());
                    outcome.pm_summary = Some(summary);
                }
            } else if outcome.new_discussion_points.is_empty() {
                let note = request.thought.trim();
                if !note.is_empty() {
                    let derived = DiscussionPoint {
                        role: role.clone(),
                        detail: note.to_string(),
                    };
                    self.discussion_log.push(derived.clone());
                    outcome.new_discussion_points.push(derived);
                }
            }
        }

        for story in &request.backlog_stories {
            let change_type = if self.backlog.contains_key(&story.id) {
                BacklogChangeType::Updated
            } else {
                BacklogChangeType::Added
            };
            self.backlog.insert(story.id.clone(), story.clone());
            outcome
                .backlog_changes
                .push(BacklogChange::new(change_type, story.clone()));
        }

        for story_id in &request.remove_story_ids {
            if let Some(removed) = self.backlog.remove(story_id) {
                outcome
                    .backlog_changes
                    .push(BacklogChange::new(BacklogChangeType::Removed, removed));
            }
        }

        if let Some(plan) = &request.sprint_plan {
            self.active_sprint = Some(plan.clone());
            outcome.sprint_plan_updated = Some(plan.clone());
        }

        if let Some(update) = &request.consensus_update {
            self.consensus.ready_for_code_changes = update.ready_for_code_changes;
            self.consensus.blockers = update.blockers.clone();
            self.consensus.notes = update.notes.clone();
            outcome.consensus_state = Some(self.consensus.clone());
        }

        if let Some(needs_input) = request.requires_user_input {
            self.awaiting_user_input = needs_input;
            outcome.awaiting_user_input = Some(needs_input);
        }

        outcome
    }

    fn generate_report(
        &self,
        request: &DeliberateThinkingRequest,
        outcome: &TeamUpdateOutcome,
    ) -> ProjectManagerReport {
        let mut bullets = Vec::new();
        let backlog_snapshot = self.ordered_backlog();
        let pm_summary = outcome
            .pm_summary
            .clone()
            .or_else(|| self.pm_summaries.last().cloned())
            .or_else(|| {
                if matches!(request.role, Some(TeamRole::ProjectManager)) {
                    let summary = request.thought.trim();
                    (!summary.is_empty()).then(|| summary.to_string())
                } else {
                    None
                }
            });

        let pm_summary_text = pm_summary
            .clone()
            .unwrap_or_else(|| "No project manager summary provided yet".to_string());
        bullets.push(format!("PM summary: {}", pm_summary_text));

        if !outcome.new_discussion_points.is_empty() {
            let discussion = outcome
                .new_discussion_points
                .iter()
                .map(|point| format!("{}: {}", point.role, point.detail))
                .collect::<Vec<_>>()
                .join("; ");
            bullets.push(format!("Discussion points: {}", discussion));
        } else if let Some(last_point) = self.discussion_log.last() {
            bullets.push(format!(
                "Discussion points: {}: {}",
                last_point.role, last_point.detail
            ));
        } else {
            bullets.push("Discussion points: none recorded yet".to_string());
        }

        if !outcome.backlog_changes.is_empty() {
            let changes = outcome
                .backlog_changes
                .iter()
                .map(|change| change.summary())
                .collect::<Vec<_>>()
                .join("; ");
            bullets.push(format!("Backlog updates: {}", changes));
        } else if backlog_snapshot.is_empty() {
            bullets.push("Backlog updates: backlog is empty".to_string());
        } else {
            let highlights = backlog_snapshot
                .iter()
                .take(3)
                .map(|item| format!("{} [{} | {}]", item.id, item.priority, item.status))
                .collect::<Vec<_>>()
                .join("; ");
            bullets.push(format!("Backlog focus: {}", highlights));
        }

        if let Some(plan) = outcome
            .sprint_plan_updated
            .as_ref()
            .or_else(|| self.active_sprint.as_ref())
        {
            let stories = if plan.committed_story_ids.is_empty() {
                "no stories committed".to_string()
            } else {
                plan.committed_story_ids.join(", ")
            };
            let participants = if plan.participants.is_empty() {
                "no participants selected".to_string()
            } else {
                plan.participants
                    .iter()
                    .map(|participant| {
                        let mut detail = participant.role.to_string();
                        if let Some(reason) = &participant.reasoning {
                            if !reason.trim().is_empty() {
                                detail.push_str(&format!(" ({})", reason));
                            }
                        }
                        if !participant.responsibilities.is_empty() {
                            detail.push_str(&format!(
                                " - {}",
                                participant.responsibilities.join(", ")
                            ));
                        }
                        detail
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            bullets.push(format!(
                "Sprint plan: {} goal '{}' lasting {} day(s); stories {}; participants {}",
                plan.sprint_name, plan.goal, plan.duration_days, stories, participants
            ));
        } else {
            bullets.push("Sprint plan: not yet defined".to_string());
        }

        let consensus = outcome
            .consensus_state
            .clone()
            .unwrap_or_else(|| self.consensus.clone());
        let blockers = if consensus.blockers.is_empty() {
            "none".to_string()
        } else {
            consensus.blockers.join("; ")
        };
        let notes = consensus
            .notes
            .as_deref()
            .filter(|note| !note.trim().is_empty())
            .unwrap_or("no additional notes");
        let waiting_on_user = outcome
            .awaiting_user_input
            .unwrap_or(self.awaiting_user_input);
        bullets.push(format!(
            "Consensus: ready_for_code_change={} blockers {} notes {} waiting_on_user={}",
            bool_to_yes(consensus.ready_for_code_changes),
            blockers,
            notes,
            bool_to_yes(waiting_on_user)
        ));

        ProjectManagerReport {
            bullets,
            pm_summary,
            new_discussion_points: outcome.new_discussion_points.clone(),
            backlog_snapshot,
            active_sprint: self.active_sprint.clone(),
            consensus,
            waiting_on_user,
        }
    }

    fn ordered_backlog(&self) -> Vec<BacklogItem> {
        let mut items: Vec<BacklogItem> = self.backlog.values().cloned().collect();
        items.sort_by(|a, b| {
            a.priority
                .rank()
                .cmp(&b.priority.rank())
                .then_with(|| a.status.rank().cmp(&b.status.rank()))
                .then_with(|| a.id.cmp(&b.id))
        });
        items
    }
}

fn bool_to_yes(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// Deliberate thinking server state
#[derive(Debug, Default)]
pub struct DeliberateThinkingState {
    pub thought_history: Vec<ThoughtData>,
    pub branches: HashMap<String, Vec<ThoughtData>>,
    pub current_branch: Option<String>,
    pub team: TeamState,
}

impl DeliberateThinkingState {
    /// Gets the current thought history (from branch or main)
    fn get_current_history(&self) -> &[ThoughtData] {
        match &self.current_branch {
            Some(branch_id) => self
                .branches
                .get(branch_id)
                .map(|v| v.as_slice())
                .unwrap_or(&self.thought_history),
            None => &self.thought_history,
        }
    }

    /// Gets the current thought history length
    fn get_history_length(&self) -> u32 {
        self.get_current_history().len() as u32
    }

    /// Handles branching logic
    fn handle_branching(&mut self, branch_from: u32, branch_id: String, thought_data: ThoughtData) {
        // Create branch if it doesn't exist
        if !self.branches.contains_key(&branch_id) {
            let branch_base: Vec<ThoughtData> = self
                .thought_history
                .iter()
                .take_while(|t| t.thought_number <= branch_from)
                .cloned()
                .collect();
            self.branches.insert(branch_id.clone(), branch_base);
        }

        // Add thought to the branch
        if let Some(branch) = self.branches.get_mut(&branch_id) {
            branch.push(thought_data);
        }

        self.current_branch = Some(branch_id);
    }

    /// Handles revision of existing thoughts
    fn handle_revision(&mut self, revises: u32, thought_data: ThoughtData) {
        match &self.current_branch {
            Some(branch_id) => {
                if let Some(branch) = self.branches.get_mut(branch_id) {
                    Self::revise_or_append(branch, revises, thought_data);
                }
            }
            None => {
                Self::revise_or_append(&mut self.thought_history, revises, thought_data);
            }
        }
    }

    /// Helper to revise a thought in a list or append if not found
    fn revise_or_append(thoughts: &mut Vec<ThoughtData>, revises: u32, thought_data: ThoughtData) {
        if let Some(thought) = thoughts.iter_mut().find(|t| t.thought_number == revises) {
            *thought = thought_data;
        } else {
            thoughts.push(thought_data);
        }
    }

    /// Adds a regular thought to the current context
    fn add_thought(&mut self, thought_data: ThoughtData) {
        match &self.current_branch {
            Some(branch_id) => {
                if let Some(branch) = self.branches.get_mut(branch_id) {
                    branch.push(thought_data);
                }
            }
            None => {
                self.thought_history.push(thought_data);
            }
        }
    }

    /// Gets all branch names
    fn get_branch_names(&self) -> Vec<String> {
        self.branches.keys().cloned().collect()
    }
}

/// Deliberate thinking server implementation
#[derive(Clone)]
pub struct DeliberateThinkingServer {
    state: Arc<Mutex<DeliberateThinkingState>>,
    tool_router: ToolRouter<Self>,
}

impl DeliberateThinkingServer {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DeliberateThinkingState::default())),
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for DeliberateThinkingServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to validate minimum values

fn validate_min_value(field_name: &str, value: u32, min: u32) -> Result<(), McpError> {
    if value < min {
        Err(create_validation_error(&format!(
            "{} must be at least {}",
            field_name, min
        )))
    } else {
        Ok(())
    }
}

fn validate_non_empty(field_name: &str, value: &str) -> Result<(), McpError> {
    if value.trim().is_empty() {
        Err(create_validation_error(&format!(
            "{} cannot be empty",
            field_name
        )))
    } else {
        Ok(())
    }
}

/// Helper function to create validation errors
fn create_validation_error(message: &str) -> McpError {
    McpError {
        code: ErrorCode(-32602),
        message: message.to_string().into(),
        data: None,
    }
}

/// Helper function to create serialization errors
fn create_serialization_error(error: impl std::fmt::Display) -> McpError {
    McpError {
        code: ErrorCode(-32603),
        message: format!("Failed to serialize response: {}", error).into(),
        data: None,
    }
}

#[tool_router]
impl DeliberateThinkingServer {
    /// Deliberate thinking tool for dynamic and reflective problem-solving
    #[tool(
        name = "deliberatethinking",
        description = "A detailed tool for dynamic and reflective problem-solving through thoughts.
This tool helps analyze problems through a flexible thinking process that can adapt and evolve.
Each thought can build on, question, or revise previous insights as understanding deepens.

When to use this tool:
- Breaking down complex problems into steps
- Planning and design with room for revision
- Analysis that might need course correction
- Problems where the full scope might not be clear initially
- Problems that require a multi-step solution
- Tasks that need to maintain context over multiple steps
- Situations where irrelevant information needs to be filtered out

Key features:
- You can adjust total_thoughts up or down as you progress
- You can question or revise previous thoughts
- You can add more thoughts even after reaching what seemed like the end
- You can express uncertainty and explore alternative approaches
- Not every thought needs to build linearly - you can branch or backtrack
- Generates a solution hypothesis
- Verifies the hypothesis based on the Chain of Thought steps
- Repeats the process until satisfied
- Provides a correct answer"
    )]

    pub async fn deliberate_thinking(
        &self,
        Parameters(request): Parameters<DeliberateThinkingRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Validate parameters
        request.validate()?;

        // Convert request to thought data (consumes the request)
        let thought_data = ThoughtData::from(request.clone());

        let mut state = self.state.lock().await;

        // Update team collaboration state
        let team_outcome = state.team.process_request(&request);

        // Process the thought based on its type
        match (
            &request.branch_from_thought,
            &request.branch_id,
            &request.revises_thought,
        ) {
            // Branching case
            (Some(branch_from), Some(branch_id), _) => {
                state.handle_branching(*branch_from, branch_id.clone(), thought_data);
            }
            // Revision case
            (_, _, Some(revises)) => {
                state.handle_revision(*revises, thought_data);
            }
            // Regular thought case
            _ => {
                state.add_thought(thought_data);
            }
        }

        // Build the project manager report summarising this step
        let pm_report = state.team.generate_report(&request, &team_outcome);

        // Create response
        let response = DeliberateThinkingResponse::new(
            &request,
            state.get_branch_names(),
            state.get_history_length(),
            pm_report,
        );

        // Log the thought for debugging
        log_thought_info(&request);

        // Serialize response
        let response_json = serde_json::to_value(response).map_err(create_serialization_error)?;

        Ok(CallToolResult::success(vec![Content::text(
            response_json.to_string(),
        )]))
    }
}

/// Logs information about the current thought
fn log_thought_info(request: &DeliberateThinkingRequest) {
    log::info!(
        "Deliberate Thinking Step {}/{}: {}",
        request.thought_number,
        request.total_thoughts,
        request.thought
    );

    if let Some(ref branch_id) = request.branch_id {
        log::info!("  Branch: {}", branch_id);
    }

    if let Some(role) = &request.role {
        log::info!("  Team role: {}", role);
    }

    if request.is_revision.unwrap_or(false) {
        if let Some(revises) = request.revises_thought {
            log::info!("  Revision of thought {}", revises);
        }
    }

    if !request.discussion_points.is_empty() {
        let highlights = request
            .discussion_points
            .iter()
            .map(|point| format!("{}: {}", point.role, point.detail))
            .collect::<Vec<_>>()
            .join("; ");
        log::info!("  Discussion points: {}", highlights);
    }

    if !request.backlog_stories.is_empty() || !request.remove_story_ids.is_empty() {
        log::info!(
            "  Backlog updates -> add/update: {}, remove: {}",
            request.backlog_stories.len(),
            request.remove_story_ids.len()
        );
    }

    if request.sprint_plan.is_some() {
        log::info!("  Sprint plan proposal included");
    }

    if let Some(consensus) = &request.consensus_update {
        log::info!(
            "  Consensus update: ready_for_code_changes={} blockers={}",
            consensus.ready_for_code_changes,
            consensus.blockers.len()
        );
    }

    if let Some(needs_input) = request.requires_user_input {
        log::info!("  Waiting on user input: {}", needs_input);
    }
}

#[tool_handler]
impl ServerHandler for DeliberateThinkingServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability::default()),
                ..Default::default()
            },
            server_info: Implementation {
                name: "deliberate-thinking-rust".to_string(),
                version: "0.1.0".to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: None,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let server = DeliberateThinkingServer::new();

    log::info!("Starting Deliberate Thinking MCP Server");

    // Run the server using stdio transport
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
