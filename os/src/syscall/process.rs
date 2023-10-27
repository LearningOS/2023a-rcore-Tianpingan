//! Process management syscalls

use core::{ptr::copy_nonoverlapping, mem::size_of};

use crate::{
    config::{MAX_SYSCALL_NUM, PAGE_SIZE},
    task::{
        change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, current_user_token, get_syscall_count, get_task_time,
    }, mm::{translated_byte_buffer, unmmap}, mm::mmap, timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
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
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    // trace!("kernel: sys_get_time");
    // let _buffer = translated_byte_buffer(current_user_token(), ts as *const u8, tz);
    
    // -1
    let us = get_time_us();
    let tv = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let buffers = translated_byte_buffer(current_user_token(), ts as *const u8, size_of::<TimeVal>());
    for buffer in buffers {
        unsafe { copy_nonoverlapping(&tv as *const TimeVal, buffer[0] as *mut TimeVal, buffer.len()) };
    }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    // trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");
    
    // -1
    let syscall_times = get_syscall_count();
    let time = my_get_time() - get_task_time();
    let task_info =  TaskInfo {
        status: TaskStatus::Running,
        syscall_times: syscall_times,
        time: time,
    };
    let buffers = translated_byte_buffer(current_user_token(), ti as *const u8, size_of::<TaskInfo>());
    for buffer in buffers {
        unsafe { copy_nonoverlapping(&task_info as *const TaskInfo, buffer[0] as *mut TaskInfo, buffer.len()) };
    }
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    // trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if port & 0x7 == 0 {
        return -1;
    }
    if port & (!(0x7)) != 0 {
        return -1;
    }
    // 向上取整
    let len = (len + PAGE_SIZE - 1) / PAGE_SIZE;
    if len == 0 {
        return 0;
    }
    mmap(current_user_token(), start, len, port.try_into().unwrap())
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    // trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    // -1
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    let len = (len + PAGE_SIZE - 1) / PAGE_SIZE;
    if len == 0 {
        return 0;
    }
    unmmap(current_user_token(), start, len)
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

pub fn my_get_time() -> usize {
    let mut time = TimeVal { sec: 0, usec: 0 };
    match sys_get_time(&mut time as *mut TimeVal, 0) {
        0 => ((time.sec & 0xffff) * 1000 + time.usec / 1000) as usize,
        _ => 0,
    }
}
