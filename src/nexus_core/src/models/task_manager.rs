use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::io::Cursor;

use crate::models::tasks::{Task, TaskError, TaskId, TaskStatus};
use crate::models::AgentStore;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::Writer;
use uuid::Uuid;

/// In-memory task manager that tracks tasks, their status, and inter-task dependencies.
#[derive(Debug, Default, Clone)]
pub struct TaskManager {
    tasks: HashMap<TaskId, Task>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn add_task<I>(
        &mut self,
        name: impl Into<String>,
        instructions: impl Into<String>,
        priority: u32,
        requirements: I,
    ) -> Result<TaskId, TaskError>
    where
        I: IntoIterator<Item = TaskId>,
    {
        let requirements: HashSet<TaskId> = requirements.into_iter().collect();

        for req in &requirements {
            if !self.tasks.contains_key(req) {
                return Err(TaskError::UnknownRequirement(*req));
            }
        }

        let task = Task::new(name, instructions, priority, requirements.clone());
        let task_id = task.id;

        for req in &requirements {
            if let Some(dependency) = self.tasks.get_mut(req) {
                dependency.dependents.insert(task_id);
            }
        }

        self.tasks.insert(task_id, task);
        Ok(task_id)
    }

    pub fn get(&self, id: &TaskId) -> Option<&Task> {
        self.tasks.get(id)
    }

    pub fn get_mut(&mut self, id: &TaskId) -> Option<&mut Task> {
        self.tasks.get_mut(id)
    }

    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values()
    }

    pub fn tasks_mut(&mut self) -> impl Iterator<Item = &mut Task> {
        self.tasks.values_mut()
    }

    pub fn mark_completed(&mut self, task_id: TaskId) -> Result<(), TaskError> {
        let ready = self.is_ready(task_id)?;
        if !ready {
            return Err(TaskError::RequirementsIncomplete(task_id));
        }

        let task = self
            .tasks
            .get_mut(&task_id)
            .ok_or(TaskError::UnknownTask(task_id))?;
        task.status = TaskStatus::Completed;
        Ok(())
    }

    pub fn is_ready(&self, task_id: TaskId) -> Result<bool, TaskError> {
        let task = self
            .tasks
            .get(&task_id)
            .ok_or(TaskError::UnknownTask(task_id))?;
        Ok(task
            .requirements
            .iter()
            .all(|req| self.tasks.get(req).is_some_and(Task::is_completed)))
    }

    pub fn assign_task(
        &mut self,
        task_id: TaskId,
        agent_id: Uuid,
        agents: &AgentStore,
    ) -> Result<(), TaskError> {
        if agents.get_agent(&agent_id).is_none() {
            return Err(TaskError::AgentNotFound(agent_id));
        }

        let task = self
            .tasks
            .get_mut(&task_id)
            .ok_or(TaskError::UnknownTask(task_id))?;
        task.assigned_to = Some(agent_id);
        Ok(())
    }

    pub fn unassign_task(&mut self, task_id: TaskId) -> Result<(), TaskError> {
        let task = self
            .tasks
            .get_mut(&task_id)
            .ok_or(TaskError::UnknownTask(task_id))?;
        task.assigned_to = None;
        Ok(())
    }

    pub fn assigned_agent(&self, task_id: TaskId) -> Result<Option<Uuid>, TaskError> {
        let task = self
            .tasks
            .get(&task_id)
            .ok_or(TaskError::UnknownTask(task_id))?;
        Ok(task.assigned_to)
    }

    /// Returns batches of tasks that can execute in parallel, ordered by priority.
    pub fn prioritized_batches(&self) -> Result<Vec<Vec<TaskId>>, TaskError> {
        #[derive(Debug, Clone, Copy, Eq, PartialEq)]
        struct ReadyTask {
            priority: u32,
            task_id: TaskId,
        }

        impl Ord for ReadyTask {
            fn cmp(&self, other: &Self) -> Ordering {
                self.priority
                    .cmp(&other.priority)
                    .then_with(|| self.task_id.cmp(&other.task_id))
            }
        }

        impl PartialOrd for ReadyTask {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
        let mut ready: BinaryHeap<ReadyTask> = BinaryHeap::new();

        for task in self.tasks.values() {
            if task.is_completed() {
                continue;
            }

            let unmet = task
                .requirements
                .iter()
                .filter(|req| self.tasks.get(req).is_some_and(|t| !t.is_completed()))
                .count();

            if unmet == 0 {
                ready.push(ReadyTask {
                    priority: task.priority,
                    task_id: task.id,
                });
            }

            in_degree.insert(task.id, unmet);
        }

        let mut batches: Vec<Vec<TaskId>> = Vec::new();
        let mut scheduled = 0usize;

        while let Some(&ReadyTask { priority, .. }) = ready.peek() {
            let mut batch = Vec::new();
            let mut to_process = Vec::new();

            while ready
                .peek()
                .is_some_and(|candidate| candidate.priority == priority)
            {
                if let Some(ReadyTask { task_id, .. }) = ready.pop() {
                    batch.push(task_id);
                    to_process.push(task_id);
                }
            }

            for task_id in to_process {
                scheduled += 1;

                if let Some(task) = self.tasks.get(&task_id) {
                    for dependent in &task.dependents {
                        if let Some(entry) = in_degree.get_mut(dependent) {
                            if *entry == 0 {
                                continue;
                            }

                            *entry -= 1;
                            if *entry == 0 {
                                if let Some(dep_task) = self.tasks.get(dependent) {
                                    if !dep_task.is_completed() {
                                        ready.push(ReadyTask {
                                            priority: dep_task.priority,
                                            task_id: *dependent,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            batches.push(batch);
        }

        let total_active_tasks = in_degree.len();
        if scheduled != total_active_tasks {
            return Err(TaskError::CycleDetected);
        }

        Ok(batches)
    }

    /// Returns the task list in XML format
    pub fn to_xml(&self) -> String {
        let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);

        writer
            .write_event(Event::Start(BytesStart::new("task_list")))
            .unwrap();

        self.tasks
            .values()
            .for_each(|task| task.to_xml(&mut writer));

        writer
            .write_event(Event::End(BytesEnd::new("task_list")))
            .unwrap();

        String::from_utf8(writer.into_inner().into_inner()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Agent, AgentStore};
    use crate::tools::ToolRegistry;

    #[test]
    fn add_task_registers_dependencies_and_dependents() {
        let mut manager = TaskManager::default();
        let root = manager
            .add_task(
                "bootstrap",
                "bootstrap instructions",
                10,
                Vec::<TaskId>::new(),
            )
            .expect("root task should be created");
        let child = manager
            .add_task("install", "install instructions", 5, [root])
            .expect("child task should be created");

        assert_ne!(root, child, "each task must receive a unique id");
        let root_ref = manager.get(&root).expect("root should exist");
        assert!(root_ref.dependents.contains(&child));

        let child_ref = manager.get(&child).expect("child should exist");
        assert!(child_ref.requirements.contains(&root));
        assert_eq!(child_ref.status, TaskStatus::Pending);
    }

    #[test]
    fn mark_completed_enforces_dependency_closure() {
        let mut manager = TaskManager::default();
        let root = manager
            .add_task("gather", "gather instructions", 1, Vec::<TaskId>::new())
            .unwrap();
        let child = manager
            .add_task("process", "process instructions", 1, [root])
            .unwrap();

        let err = manager
            .mark_completed(child)
            .expect_err("child should not complete while dependency pending");
        assert!(matches!(err, TaskError::RequirementsIncomplete(_)));

        manager.mark_completed(root).unwrap();
        assert!(manager.is_ready(child).unwrap());
        manager.mark_completed(child).unwrap();
        assert!(manager.get(&child).unwrap().is_completed());
    }

    #[test]
    fn prioritized_batches_respect_priority_groups() {
        let mut manager = TaskManager::default();

        let a = manager
            .add_task("a", "instructions a", 5, Vec::<TaskId>::new())
            .expect("task a");
        let b = manager
            .add_task("b", "instructions b", 10, Vec::<TaskId>::new())
            .expect("task b");
        let c = manager
            .add_task("c", "instructions c", 7, [a, b])
            .expect("task c");
        let d = manager
            .add_task("d", "instructions d", 3, [a])
            .expect("task d");

        let batches = manager.prioritized_batches().expect("schedule");
        assert_eq!(batches.len(), 4);
        assert_eq!(batches[0], vec![b]);
        assert_eq!(batches[1], vec![a]);
        assert_eq!(batches[2], vec![c]);
        assert_eq!(batches[3], vec![d]);
    }

    #[test]
    fn prioritized_batches_detects_cycles() {
        let mut manager = TaskManager::default();
        let a = manager
            .add_task("a", "instructions a", 1, Vec::<TaskId>::new())
            .expect("task a");
        let b = manager
            .add_task("b", "instructions b", 1, [a])
            .expect("task b");

        manager.get_mut(&a).unwrap().requirements.insert(b);
        manager.get_mut(&b).unwrap().dependents.insert(a);

        let err = manager
            .prioritized_batches()
            .expect_err("cycle should be reported");
        assert!(matches!(err, TaskError::CycleDetected));
    }

    #[test]
    fn add_task_rejects_unknown_requirement() {
        let mut manager = TaskManager::default();
        let bogus = Uuid::new_v4();
        let err = manager
            .add_task("failing", "instructions failing", 1, [bogus])
            .expect_err("should refuse unknown dependencies");
        assert!(matches!(err, TaskError::UnknownRequirement(_)));
    }

    #[test]
    fn assign_task_validates_agent() {
        let mut manager = TaskManager::default();
        let task_id = manager
            .add_task(
                "orchestrate",
                "instructions orchestrate",
                1,
                Vec::<TaskId>::new(),
            )
            .expect("task created");

        let mut agents = AgentStore::new();
        let agent_id = agents.add_agent(Agent::new(
            "runner",
            "",
            "",
            Vec::new(),
            ToolRegistry::new(),
        ));

        manager
            .assign_task(task_id, agent_id, &agents)
            .expect("assignment succeeds");
        assert_eq!(manager.assigned_agent(task_id).unwrap(), Some(agent_id));

        manager.unassign_task(task_id).unwrap();
        assert!(manager.assigned_agent(task_id).unwrap().is_none());
    }

    #[test]
    fn assign_task_rejects_unknown_agent() {
        let mut manager = TaskManager::default();
        let task_id = manager
            .add_task(
                "orchestrate",
                "instructions orchestrate",
                1,
                Vec::<TaskId>::new(),
            )
            .expect("task created");

        let agents = AgentStore::new();
        let bogus = Uuid::new_v4();
        let err = manager
            .assign_task(task_id, bogus, &agents)
            .expect_err("should reject unknown agent");
        assert!(matches!(err, TaskError::AgentNotFound(_)));
    }
}
