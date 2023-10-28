//! Process management syscalls

use core::{
    mem::{self, size_of},
    slice,
};

use crate::{
    config::MAX_SYSCALL_NUM,
    mm::translated_byte_buffer,
    mm::{VirtAddr, VirtPageNum},
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next, get_syscall_count,
        get_task_time, mmap, munmap, suspend_current_and_run_next, TaskStatus,
    },
    timer::get_time_us,
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
pub unsafe fn serialize_row<T: Sized>(src: &T) -> &[u8] {
    slice::from_raw_parts((src as *const T) as *const u8, mem::size_of::<T>())
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, tz: usize) -> isize {
    trace!("kernel: sys_get_time------, tz = {}", tz);
    let us = get_time_us();
    let tv: TimeVal = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let tv = unsafe { self::serialize_row(&tv) };
    let buffers =
        translated_byte_buffer(current_user_token(), ts as *const u8, size_of::<TimeVal>());
    trace!("size: {:?}", buffers.len());
    for buffer in buffers {
        trace!("buffer: {:?}", buffer.len());
        // let src = &ts[0..109];
        // tv.as
        buffer.copy_from_slice(tv);
        // unsafe { copy_nonoverlapping(&tv as *const TimeVal, buffer[0] as *mut TimeVal, buffer.len()) };
        // trace!("after: buffer: {:?}", buffer);
    }

    trace!("kernel-end: sys_get_time------");
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");

    // -1
    let syscall_times = get_syscall_count();
    let time = my_get_time() - get_task_time();
    let task_info = TaskInfo {
        status: TaskStatus::Running,
        syscall_times: syscall_times,
        time: time,
    };
    let task_info = unsafe { self::serialize_row(&task_info) };
    let buffers =
        translated_byte_buffer(current_user_token(), ti as *const u8, size_of::<TaskInfo>());
    trace!("task-info, buffer_size: {}", buffers.len());
    for buffer in buffers {
        trace!("task-info, buffer: {:?}", buffer.len());
        buffer.copy_from_slice(task_info);

        // trace!("after-task-info, buffer: {:?}", buffer);
        // unsafe { copy_nonoverlapping(&task_info as *const TaskInfo, buffer[0] as *mut TaskInfo, buffer.len()) };
    }
    trace!("kernel-end: sys_task_info NOT IMPLEMENTED YET!");
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    let start_vaddr: VirtAddr = start.into();
    if !start_vaddr.aligned() {
        debug!("map fail don't aligned");
        return -1;
    }
    if port & !0x7 != 0 || port & 0x7 == 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let end_vaddr: VirtAddr = (start + len).into();
    let start_vpn: VirtPageNum = start_vaddr.into();
    let end_vpn: VirtPageNum = (end_vaddr).ceil();

    mmap(start_vpn, end_vpn, port)

    // trace!("sys_mmap: start: {}, len: {}, port: {}", start, len, port);
    // if start % PAGE_SIZE != 0 {
    //     return -1;
    // }
    // if port & 0x7 == 0 {
    //     return -1;
    // }
    // if port & (!(0x7)) != 0 {
    //     return -1;
    // }
    // // 向上取整
    // let len = ((len + PAGE_SIZE - 1) / PAGE_SIZE) * PAGE_SIZE;
    // if len == 0 {
    //     return 0;
    // }
    // // KERNEL_SPACE;
    // mmap(current_user_token(), start, len, port.try_into().unwrap())
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    let start_vaddr: VirtAddr = start.into();
    if !start_vaddr.aligned() {
        debug!("unmap fail don't aligned");
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let end_vaddr: VirtAddr = (start + len).into();
    let start_vpn: VirtPageNum = start_vaddr.into();
    let end_vpn: VirtPageNum = (end_vaddr).ceil();

    munmap(start_vpn, end_vpn)
    // trace!("sys_mnmmap: start: {}, len: {}", start, len);
    // // -1
    // if start % PAGE_SIZE != 0 {
    //     return -1;
    // }
    // let len = ((len + PAGE_SIZE - 1) / PAGE_SIZE) * PAGE_SIZE;
    // if len == 0 {
    //     return 0;
    // }
    // unmmap(current_user_token(), start, len)
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
    // let mut time = TimeVal { sec: 0, usec: 0 };
    // match sys_get_time(&mut time as *mut TimeVal, 0) {
    //     0 => ((time.sec & 0xffff) * 1000 + time.usec / 1000) as usize,
    //     _ => 0,
    // }
    let us = get_time_us();
    let time: TimeVal = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    ((time.sec & 0xffff) * 1000 + time.usec / 1000) as usize
}
