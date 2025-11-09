use std::collections::HashSet;

use crate::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier assigned to each task when it is registered with the manager.
pub type TaskId = Uuid;

/// Represents the current lifecycle status of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Completed,
}

/// Metadata for a task registered with the manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub instructions: String,
    pub priority: u32,
    pub status: TaskStatus,
    /// Tasks that **must** be completed before this task can be executed.
    pub requirements: HashSet<TaskId>,
    /// Tasks that list this task as a dependency.
    pub dependents: HashSet<TaskId>,
    /// Agent currently responsible for this task, if any.
    pub assigned_to: Option<Uuid>,
}

impl Task {
    pub fn to_xml(&self, writer: &mut quick_xml::Writer<std::io::Cursor<Vec<u8>>>) {
        use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
        writer
            .write_event(Event::Start(BytesStart::new("task")))
            .unwrap();

        // Write task id
        writer
            .write_event(Event::Start(BytesStart::new("id")))
            .unwrap();
        writer
            .write_event(Event::Text(BytesText::new(&self.id.to_string())))
            .unwrap();
        writer.write_event(Event::End(BytesEnd::new("id"))).unwrap();

        // Write task name
        writer
            .write_event(Event::Start(BytesStart::new("name")))
            .unwrap();
        writer
            .write_event(Event::Text(BytesText::new(&self.name)))
            .unwrap();
        writer
            .write_event(Event::End(BytesEnd::new("name")))
            .unwrap();

        writer
            .write_event(Event::Start(BytesStart::new("instructions")))
            .unwrap();
        writer
            .write_event(Event::Text(BytesText::new(&self.instructions)))
            .unwrap();
        writer
            .write_event(Event::End(BytesEnd::new("instructions")))
            .unwrap();

        writer
            .write_event(Event::End(BytesEnd::new("task")))
            .unwrap();
    }

    pub fn new(
        name: impl Into<String>,
        instructions: impl Into<String>,
        priority: u32,
        requirements: HashSet<TaskId>,
    ) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            name: name.into(),
            instructions: instructions.into(),
            priority,
            status: TaskStatus::Pending,
            requirements,
            dependents: HashSet::new(),
            assigned_to: None,
        }
    }

    pub fn is_completed(&self) -> bool {
        matches!(self.status, TaskStatus::Completed)
    }
}

pub struct TaskAssignment {
    pub task_id: TaskId,
    pub agent_id: Uuid,
}

impl TaskAssignment {
    pub fn to_output_format() -> JsonSchema {
        JsonSchema::new("object")
            .with_properties(serde_json::json!({
                "task_assignments": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "task_id": {
                                "type": "string",
                                "description": "UUID of the task to be assigned"
                            },
                            "agent_id": {
                                "type": "string",
                                "description": "UUID of the agent to assign the task to"
                            }
                        },
                        "required": ["task_id", "agent_id"],
                        "additionalProperties": false
                    }
                }
            }))
            .with_required(vec!["task_assignments".to_string()])
            .with_additional_properties(false)
    }
}

/// Represents a task definition for decomposition output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    pub name: String,
    #[serde(default)]
    pub instructions: String,
    pub priority: u32,
    pub requirements: Vec<String>,
}

/// Container for multiple task definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDecomposition {
    pub tasks: Vec<TaskDefinition>,
}

impl TaskDecomposition {
    pub fn to_output_format() -> JsonSchema {
        JsonSchema::new("object")
            .with_properties(serde_json::json!({
                "tasks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Descriptive name of the task"
                            },
                            "instructions": {
                                "type": "string",
                                "description": "Detailed instructions for the agent to execute the task"
                            },
                            "priority": {
                                "type": "integer",
                                "description": "Priority level from 1-10",
                                "minimum": 1,
                                "maximum": 10
                            },
                            "requirements": {
                                "type": "array",
                                "description": "List of task names that must be completed before this task",
                                "items": {
                                    "type": "string"
                                }
                            }
                        },
                        "required": ["name", "instructions", "priority", "requirements"],
                        "additionalProperties": false
                    }
                }
            }))
            .with_required(vec!["tasks".to_string()])
            .with_additional_properties(false)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("unknown task: {0}")]
    UnknownTask(TaskId),
    #[error("requirement not found: {0}")]
    UnknownRequirement(TaskId),
    #[error("requirements not completed for task: {0}")]
    RequirementsIncomplete(TaskId),
    #[error("cyclic dependency detected")]
    CycleDetected,
    #[error("agent not found: {0}")]
    AgentNotFound(Uuid),
}
