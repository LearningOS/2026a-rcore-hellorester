//! Semaphore

// AI assistance: chapter 8 changes developed with Codex; see reports/lab5.md.
use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_process, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
    resource_id: Option<usize>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
            resource_id: None,
        }
    }

    /// Create a semaphore tracked by its process resource ID.
    pub fn new_tracked(res_count: usize, id: usize) -> Self {
        let mut semaphore = Self::new(res_count);
        semaphore.resource_id = Some(id);
        semaphore
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                let tid = task.inner_exclusive_access().res.as_ref().unwrap().tid;
                record_semaphore_up(self.resource_id, Some(tid));
                wakeup_task(task);
            }
        } else {
            record_semaphore_up(self.resource_id, None);
        }
    }

    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        record_semaphore_request(self.resource_id);
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        } else {
            record_semaphore_acquired(self.resource_id);
        }
    }
}


fn current_tid() -> usize {
    current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid
}

fn record_semaphore_request(id: Option<usize>) {
    if let Some(id) = id {
        let tid = current_tid();
        let process = current_process();
        let mut inner = process.inner_exclusive_access();
        inner.ensure_deadlock_thread(tid);
        inner.semaphore_need[tid][id] = 1;
    }
}

fn record_semaphore_acquired(id: Option<usize>) {
    if let Some(id) = id {
        let tid = current_tid();
        let process = current_process();
        let mut inner = process.inner_exclusive_access();
        inner.ensure_deadlock_thread(tid);
        inner.semaphore_available[id] -= 1;
        inner.semaphore_allocation[tid][id] += 1;
        inner.semaphore_need[tid][id] = 0;
    }
}

fn record_semaphore_up(id: Option<usize>, next_tid: Option<usize>) {
    if let Some(id) = id {
        let tid = current_tid();
        let process = current_process();
        let mut inner = process.inner_exclusive_access();
        inner.ensure_deadlock_thread(tid);
        // A producer may signal without having performed down itself.
        if inner.semaphore_allocation[tid][id] > 0 {
            inner.semaphore_allocation[tid][id] -= 1;
        }
        if let Some(next_tid) = next_tid {
            inner.ensure_deadlock_thread(next_tid);
            inner.semaphore_allocation[next_tid][id] += 1;
            inner.semaphore_need[next_tid][id] = 0;
        } else {
            inner.semaphore_available[id] += 1;
        }
    }
}
