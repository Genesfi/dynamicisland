//! TodoList, Weekly Planner & Task Manager Engine
//!
//! Provides persistent task storage (`tasks.json`), calendar schedule tracking,
//! and interactive task management for Dynamic Island's Today Dashboard and TodoList modules.

use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskItem {
    pub id: u64,
    pub title: String,
    pub time_range: String,
    pub completed: bool,
    pub category: u8, // 0: Meeting (Blue), 1: Task (Green), 2: Deadline (Red), 3: Event (Amber)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalendarView {
    Week,
    Month,
}

impl CalendarView {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            CalendarView::Week => "Week",
            CalendarView::Month => "Month",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskData {
    pub tasks: Vec<TaskItem>,
    pub view: CalendarView,
    pub month_offset: i32,
}

impl Default for TaskData {
    fn default() -> Self {
        Self {
            tasks: vec![
                TaskItem {
                    id: 1,
                    title: "Design review".to_string(),
                    time_range: "11:00–13:00".to_string(),
                    completed: false,
                    category: 0,
                },
                TaskItem {
                    id: 2,
                    title: "Weekly update".to_string(),
                    time_range: "14:00–15:00".to_string(),
                    completed: true,
                    category: 1,
                },
                TaskItem {
                    id: 3,
                    title: "Reply to Dina".to_string(),
                    time_range: "16:30–17:00".to_string(),
                    completed: true,
                    category: 0,
                },
                TaskItem {
                    id: 4,
                    title: "Code review PR #42".to_string(),
                    time_range: "17:30–18:30".to_string(),
                    completed: false,
                    category: 2,
                },
                TaskItem {
                    id: 5,
                    title: "Evening workout".to_string(),
                    time_range: "19:00–20:00".to_string(),
                    completed: false,
                    category: 3,
                },
            ],
            view: CalendarView::Week,
            month_offset: 0,
        }
    }
}

#[derive(Debug)]
pub struct TaskManager {
    pub data: TaskData,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::load()
    }
}

impl TaskManager {
    pub fn load() -> Self {
        let data = Self::read_data().unwrap_or_default();
        Self { data }
    }

    pub fn save(&self) {
        let _ = Self::write_data(&self.data);
    }

    /// Toggles task completion by visible index
    pub fn toggle_task(&mut self, index: usize) {
        if index < self.data.tasks.len() {
            self.data.tasks[index].completed = !self.data.tasks[index].completed;
            self.save();
        }
    }

    /// Adds a custom task
    pub fn add_task(&mut self, title: String, time_range: String, category: u8) {
        let next_id = self.data.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        self.data.tasks.push(TaskItem {
            id: next_id,
            title,
            time_range,
            completed: false,
            category,
        });
        self.save();
    }

    /// Deletes a task by index
    pub fn delete_task(&mut self, index: usize) {
        if index < self.data.tasks.len() {
            self.data.tasks.remove(index);
            self.save();
        }
    }

    /// Adds a new quick task
    #[allow(dead_code)]
    pub fn add_quick_task(&mut self) {
        let next_id = self.data.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let presets = [
            ("Team retro sync", "10:00–11:00", 0),
            ("Submit progress report", "13:00–14:00", 2),
            ("Client feedback call", "15:30–16:00", 0),
            ("Refactor Direct2D shaders", "16:30–17:30", 1),
            ("Walk in the park", "18:00–19:00", 3),
        ];
        let preset_idx = (next_id as usize) % presets.len();
        let (title, time_range, cat) = presets[preset_idx];

        self.data.tasks.push(TaskItem {
            id: next_id,
            title: title.to_string(),
            time_range: time_range.to_string(),
            completed: false,
            category: cat,
        });
        self.save();
    }

    /// Toggles calendar view (Week <-> Month)
    pub fn toggle_view(&mut self) {
        self.data.view = match self.data.view {
            CalendarView::Week => CalendarView::Month,
            CalendarView::Month => CalendarView::Week,
        };
        self.save();
    }

    /// Navigates displayed month forward/backward
    pub fn cycle_month(&mut self, forward: bool) {
        if forward {
            self.data.month_offset = (self.data.month_offset + 1).min(12);
        } else {
            self.data.month_offset = (self.data.month_offset - 1).max(-12);
        }
        self.save();
    }

    /// Returns (completed_tasks, total_tasks)
    #[allow(dead_code)]
    pub fn stats(&self) -> (usize, usize) {
        let total = self.data.tasks.len();
        let completed = self.data.tasks.iter().filter(|t| t.completed).count();
        (completed, total)
    }

    /// Returns the next pending uncompleted task
    #[allow(dead_code)]
    pub fn next_pending_task(&self) -> Option<&TaskItem> {
        self.data.tasks.iter().find(|t| !t.completed)
    }

    fn get_file_path() -> PathBuf {
        if let Ok(mut p) = std::env::current_exe() {
            p.pop();
            let exe_json = p.join("tasks.json");
            if exe_json.exists() {
                return exe_json;
            }
            let repo_json = p.join("..").join("..").join("tasks.json");
            if repo_json.exists() {
                if let Ok(c) = repo_json.canonicalize() {
                    return c;
                }
            }
            return exe_json;
        }
        PathBuf::from("tasks.json")
    }

    fn read_data() -> Option<TaskData> {
        let path = Self::get_file_path();
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn write_data(data: &TaskData) -> Result<(), std::io::Error> {
        let path = Self::get_file_path();
        let json = serde_json::to_string_pretty(data)?;
        fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_operations() {
        let mut mgr = TaskManager {
            data: TaskData::default(),
        };
        let (completed, total) = mgr.stats();
        assert_eq!(total, 5);
        assert_eq!(completed, 2);

        // Toggle task 0
        mgr.toggle_task(0);
        let (c2, _) = mgr.stats();
        assert_eq!(c2, 3);

        // Next pending task should now be task 3
        let next = mgr.next_pending_task().unwrap();
        assert_eq!(next.title, "Code review PR #42");

        // Toggle view
        mgr.toggle_view();
        assert_eq!(mgr.data.view, CalendarView::Month);
    }
}
