//! Process management syscalls
// AI assistance: chapter 5 changes developed with Codex; see reports/lab3.md.
use alloc::sync::Arc;
use crate::config::PAGE_SIZE;
use crate::mm::MapPermission;
use crate::mm::translated_byte_buffer;
use crate::timer::get_time_us;
use crate::task::TaskControlBlock;
use crate::{
    loader::get_app_data_by_name,
    mm::{translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// Write the current time to a possibly cross-page user buffer.
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    let time = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &time as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    let buffers = translated_byte_buffer(
        current_user_token(),
        _ts as *const u8,
        bytes.len(),
    );
    let mut offset = 0;
    for buffer in buffers {
        let len = buffer.len();
        buffer.copy_from_slice(&bytes[offset..offset + len]);
        offset += len;
    }
    0
}

/// Map user memory with the requested permissions.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    if _start % PAGE_SIZE != 0 || _port & !7 != 0 || _port & 7 == 0 {
        return -1;
    }
    if _start >= (1usize << 38) {
        return -1;
    }
    if _len == 0 {
        return 0;
    }
    let rounded_len = match _len.checked_add(PAGE_SIZE - 1) {
        Some(len) => len / PAGE_SIZE * PAGE_SIZE,
        None => return -1,
    };
    let end = match _start.checked_add(rounded_len) {
        Some(end) if end <= (1usize << 38) => end,
        _ => return -1,
    };
    let mut permission = MapPermission::U;
    if _port & 1 != 0 {
        permission |= MapPermission::R;
    }
    if _port & 2 != 0 {
        permission |= MapPermission::W;
    }
    if _port & 4 != 0 {
        permission |= MapPermission::X;
    }
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if inner.memory_set.map_region(_start, end, permission) {
        0
    } else {
        -1
    }
}

/// Unmap user memory, rounding the length up to whole pages.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    if _start % PAGE_SIZE != 0 || _start >= (1usize << 38) {
        return -1;
    }

    if _len == 0 {
        return 0;
    }

    let rounded_len = match _len.checked_add(PAGE_SIZE - 1) {
        Some(len) => len / PAGE_SIZE * PAGE_SIZE,
        None => return -1,
    };

    let end = match _start.checked_add(rounded_len) {
        Some(end) if end <= (1usize << 38) => end,
        _ => return -1,
    };

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if inner.memory_set.unmap_region(_start, end) {
        0
    } else {
        -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    let path = translated_str(current_user_token(),_path);
    let elf_data = match get_app_data_by_name(path.as_str()) {
        Some(data) => data,
        None => return -1,
    };
    let parent = current_task().unwrap();
    let child = Arc::new(TaskControlBlock::new(elf_data));
    let pid = child.getpid();
    child.inner_exclusive_access().parent = Some(Arc::downgrade(&parent));
    parent.inner_exclusive_access().children.push(Arc::clone(&child));
    add_task(child);
    pid as isize
}

/// Set the current process priority, which must be as least 2.
pub fn sys_set_priority(_prio: isize) -> isize {
    if _prio < 2 {
        return -1;
    }
    let task = current_task().unwrap();
    task.inner_exclusive_access().priority = _prio as usize;
    _prio
}
