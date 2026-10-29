// AI assistance: chapter 8 changes developed with Codex; see reports/lab5.md.
use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// Create a mutex and initialize its resource record.
pub fn sys_mutex_create(blocking: bool) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = inner.mutex_list.iter().position(|item| item.is_none())
        .unwrap_or(inner.mutex_list.len());
    let mutex: Arc<dyn Mutex> = if blocking {
        Arc::new(MutexBlocking::new_tracked(id))
    } else {
        Arc::new(MutexSpin::new_tracked(id))
    };
    if id == inner.mutex_list.len() {
        inner.mutex_list.push(Some(mutex));
    } else {
        inner.mutex_list[id] = Some(mutex);
    }
    let count = inner.mutex_list.len();
    inner.mutex_available.resize(count, 0);
    inner.mutex_available[id] = 1;
    for row in &mut inner.mutex_allocation { row.resize(count, 0); }
    for row in &mut inner.mutex_need { row.resize(count, 0); }
    id as isize
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let mutex = match inner.mutex_list.get(mutex_id).and_then(|item| item.as_ref()) {
        Some(mutex) => Arc::clone(mutex),
        None => return -1,
    };
    inner.ensure_deadlock_thread(tid);
    inner.mutex_need[tid][mutex_id] = 1;
    if inner.deadlock_detect_enabled && !inner.deadlock_state_is_safe() {
        inner.mutex_need[tid][mutex_id] = 0;
        return -0xDEAD;
    }
    drop(inner);
    drop(process);
    mutex.lock();
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let mutex = match inner.mutex_list.get(mutex_id).and_then(|item| item.as_ref()) {
        Some(mutex) => Arc::clone(mutex),
        None => return -1,
    };
    inner.ensure_deadlock_thread(tid);
    if inner.mutex_allocation[tid][mutex_id] == 0 {
        return -1;
    }
    drop(inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    if res_count > isize::MAX as usize { return -1; }
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = inner.semaphore_list.iter().position(|item| item.is_none()).unwrap_or(inner.semaphore_list.len());
    let semaphore = Some(Arc::new(Semaphore::new_tracked(res_count, id)));
    if id == inner.semaphore_list.len() {
        inner.semaphore_list.push(semaphore);
    } else {
        inner.semaphore_list[id] = semaphore;
    }
    let count = inner.semaphore_list.len();
    inner.semaphore_available.resize(count, 0);
    inner.semaphore_available[id] = res_count;
    for row in &mut inner.semaphore_allocation {
        row.resize(count, 0);
    }
    for row in &mut inner.semaphore_need {
        row.resize(count,0);
    }
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let process = current_process();
    let inner = process.inner_exclusive_access();
    let sem = match inner.semaphore_list.get(sem_id).and_then(|item| item.as_ref()) {
        Some(sem) => Arc::clone(sem),
        None => return -1,
    };
    drop(inner);
    drop(process);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let sem = match inner.semaphore_list.get(sem_id).and_then(|item| item.as_ref()) {
        Some(sem) => Arc::clone(sem),
        None => return -1,
    };
    inner.ensure_deadlock_thread(tid);
    inner.semaphore_need[tid][sem_id] = 1;
    if inner.deadlock_detect_enabled && !inner.deadlock_state_is_safe() {
        inner.semaphore_need[tid][sem_id] = 0;
        return -0xDEAD;
    }
    drop(inner);
    drop(process);
    sem.down();
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// Enable or disable deadlock detection for the current process.
pub fn sys_enable_deadlock_detect(_enabled: usize) -> isize {
    if _enabled > 1 {
        return -1;
    }

    let process = current_process();
    let mut inner = process.inner_exclusive_access();

    if _enabled == 1 && !inner.deadlock_state_is_safe() {
        return -1;
    }

    inner.deadlock_detect_enabled = _enabled == 1;
    0
}
