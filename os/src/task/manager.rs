//!Implementation of [`TaskManager`]
// AI assistance: chapter 5 changes developed with Codex; see reports/lab3.md.
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A stride scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        const BIG_STRIDE: u64 = 1 << 32;

        let mut best_index = 0;
        let mut best_stride =
            self.ready_queue.front()?.inner_exclusive_access().stride;

        for (index, task) in self.ready_queue.iter().enumerate().skip(1) {
            let stride = task.inner_exclusive_access().stride;

            // Compare wrapping counters within half of the u64 range.
            if (stride.wrapping_sub(best_stride) as i64) < 0 {
                best_index = index;
                best_stride = stride;
            }
        }

        let task = self.ready_queue.remove(best_index).unwrap();
        {
            let mut inner = task.inner_exclusive_access();
            let pass = (BIG_STRIDE / inner.priority as u64).max(1);
            inner.stride = inner.stride.wrapping_add(pass);
        }

        Some(task)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue.
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}
