//! Mutex (spin-like and blocking(sleep))

// AI assistance: chapter 8 changes developed with Codex; see reports/lab5.md.
use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_process, current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self);
    /// Unlock the mutex
    fn unlock(&self);
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
    resource_id: Option<usize>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new() -> Self {
        Self {
            locked: unsafe { UPSafeCell::new(false) },
            resource_id: None,
        }
    }
}

impl MutexSpin {
    /// Create a mutex tracked by its process resource ID.
    pub fn new_tracked(id: usize) -> Self {
        let mut mutex = Self::new();
        mutex.resource_id = Some(id);
        mutex
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) {
        trace!("kernel: MutexSpin::lock");
        record_mutex_request(self.resource_id);
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                record_mutex_acquired(self.resource_id, current_tid());
                return;
            }
        }
    }

    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let mut locked = self.locked.exclusive_access();
        *locked = false;
        record_mutex_released(self.resource_id, None);
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
    resource_id: Option<usize>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new() -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
            resource_id: None,
        }
    }

    /// Create a mutex tracked by its process resource ID.
    pub fn new_tracked(id: usize) -> Self {
        let mut mutex = Self::new();
        mutex.resource_id = Some(id);
        mutex
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) {
        trace!("kernel: MutexBlocking::lock");
        record_mutex_request(self.resource_id);
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
            record_mutex_acquired(self.resource_id, current_tid());
        }
    }

    /// unlock the blocking mutex
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            let tid = waking_task.inner_exclusive_access().res.as_ref().unwrap().tid;
            record_mutex_released(self.resource_id, Some(tid));
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
            record_mutex_released(self.resource_id, None);
        }
    }
}


fn current_tid() -> usize {
    current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid
}

fn record_mutex_request(id: Option<usize>) {
    if let Some(id) = id {
        let tid = current_tid();
        let process = current_process();
        let mut inner = process.inner_exclusive_access();
        inner.ensure_deadlock_thread(tid);
        inner.mutex_need[tid][id] = 1;
    }
}

fn record_mutex_acquired(id: Option<usize>, tid: usize) {
    if let Some(id) = id {
        let process = current_process();
        let mut inner = process.inner_exclusive_access();
        inner.ensure_deadlock_thread(tid);
        inner.mutex_available[id] -= 1;
        inner.mutex_allocation[tid][id] += 1;
        inner.mutex_need[tid][id] = 0;
    }
}

fn record_mutex_released(id: Option<usize>, next_tid: Option<usize>) {
    if let Some(id) = id {
        let tid = current_tid();
        let process = current_process();
        let mut inner = process.inner_exclusive_access();
        inner.ensure_deadlock_thread(tid);
        inner.mutex_allocation[tid][id] -= 1;

        if let Some(next_tid) = next_tid {
            inner.ensure_deadlock_thread(next_tid);
            inner.mutex_allocation[next_tid][id] += 1;
            inner.mutex_need[next_tid][id] = 0;
        } else {
            inner.mutex_available[id] += 1;
        }
    }
}
