//! Process management syscalls
// AI assistance: implementation developed with Codex; see reports/lab2.md.
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next};
use crate::mm::{translated_byte_buffer, PageTable, PTEFlags, VirtAddr};
use crate::task::current_user_token;
use crate::timer::get_time_us;
use crate::config::PAGE_SIZE;
use crate::mm::MapPermission;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    let time =  TimeVal {
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

/// Read or write a user byte, or query syscall counts.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    match _trace_request {
        0 | 1 => {
            let va = VirtAddr::from(_id);
            if usize::from(va) != _id {
                return -1;
            }
            let page_table = PageTable::from_token(current_user_token());
            let pte = match page_table.translate(va.floor()) {
                Some(pte) => pte,
                None => return -1,
            };
            if !pte.is_valid() || !pte.flags().contains(PTEFlags::U) {
                return -1;
            }
            if _trace_request == 0 {
                if !pte.readable() {
                    return -1;
                }
                pte.ppn().get_bytes_array()[va.page_offset()] as isize
            } else {
                if !pte.writable() {
                    return -1;
                }
                pte.ppn().get_bytes_array()[va.page_offset()] = _data as u8;
                0
            }
        }
        2 => crate::task::get_syscall_count(_id),
        _ => -1,
    }
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
    if crate::task::map_current_task(_start, end, permission) {
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

    if crate::task::unmap_current_task(_start, end) {
        0
    } else {
        -1
    }
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
