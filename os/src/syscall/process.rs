//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    config::{PAGE_SIZE, MAX_SYSCALL_NUM},
    loader::get_app_data_by_name,
    mm::{translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskStatus,
        syscall_times_query,
        running_time_query, map_inner
    },
    timer::get_time_us,
    mm::*, 
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

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    unsafe {
        let pt = PageTable::from_token(current_user_token());
        let _ts_va = VirtAddr::from(_ts as usize);
        let _ts_vpn = _ts_va.floor();
        let _ts_ppn = pt.translate(_ts_vpn).unwrap().ppn();
        let _ts_1 = ((*(&_ts_ppn)).0 * PAGE_SIZE + _ts_va.page_offset()) as *mut u8 as *mut TimeVal;
        // println!("DEBUG: get_time_us={}", us);
        *_ts_1 = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        }; /* NOT CORRECT IN CHAPTER 4 */
    }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    unsafe {
        let pt = PageTable::from_token(current_user_token());
        let _ti_va = VirtAddr::from(_ti as usize);
        let _ti_vpn = _ti_va.floor();
        let _ti_ppn = pt.translate(_ti_vpn).unwrap().ppn();
        let _ti_1 = ((*(&_ti_ppn)).0 * PAGE_SIZE + _ti_va.page_offset()) as *mut u8 as *mut TaskInfo;
        (*_ti_1).status = TaskStatus::Running;
        (*_ti_1).syscall_times = syscall_times_query();
        (*_ti_1).time = running_time_query();
    } /* NOT CORRECT IN CHAPTER 4 */
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    // println!("DEBUG: sys_mmap _start={} _len={} _port={}", _start, _len, _port);
    if _port & 0x7 == 0 || _port & !0x7 != 0 || _start % PAGE_SIZE != 0 {
        // println!("_port={}, _start={} Err!", _port, _start);
        -1
    } else {
        let pt = PageTable::from_token(current_user_token());

        let mut binder = _start;
        let end = _start + _len - 1;
        let flag_w = _port & 0x2;
        let flag_r = _port & 0x1;
        let flag_x = _port & 0x4;

        let mut other_flags = PTEFlags::empty();
        if flag_w != 0 {
            other_flags = other_flags | PTEFlags::W;
        }
        if flag_r != 0 {
            other_flags = other_flags | PTEFlags::R;
        }
        if flag_x != 0 {
            other_flags = other_flags | PTEFlags::X;
        }
        // Check
        while binder < ( end / PAGE_SIZE + 1 ) * PAGE_SIZE {
            // let _vpn = binder / PAGE_SIZE;
            // println!("Checking binder = {}, end = {}", binder, end);
            let virt_page_num = VirtAddr(binder).into();
            if let Some (ppn_exists) = pt.translate(virt_page_num) {
                // println!("VirPG has PhyPG onto! and ppn is {}", ppn_exists.bits);
                if ppn_exists.bits != 0 {
                    return -1;
                }
            }
            binder = binder + PAGE_SIZE;
        }
        let mut binder = _start;
        // Allocate
        while binder < ( end / PAGE_SIZE + 1 ) * PAGE_SIZE {
            // let _vpn = binder / PAGE_SIZE;
            let virt_page_num: VirtPageNum = VirtAddr(binder).into();
            let mut ppn = PhysPageNum(0);
            if let Some(ppn_wrapper) = frame_alloc() {
                ppn = ppn_wrapper.ppn;
            }
            // println!("binder: {:#x}, virt_page_num.0: {:#x}, ppn.0: {:#x}", binder, virt_page_num.0, ppn.0);
            // println!("DEBUG: Map: vpn.0={}, ppn.0={}.", virt_page_num.0, ppn.0);
            map_inner(virt_page_num, ppn, PTEFlags::U | other_flags);

            if let Some (ppn_exists) = pt.translate(virt_page_num) {
                if ppn_exists.bits == 0 {
                    // println!("ppn is 0, Map failed!");
                }
            }

            binder = binder + PAGE_SIZE;
        }
        0
    }

    // trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    // -1
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    // println!("DEBUG: sys_munmap _start={} _len={}", _start, _len);
    if _start % PAGE_SIZE != 0 {
        -1
    } else {
        let mut pt = PageTable::from_token(current_user_token());

        let mut binder = _start;
        let end = _start + _len - 1;
        // Check
        while binder < ( end / PAGE_SIZE + 1 ) * PAGE_SIZE {
            // let _vpn = binder / PAGE_SIZE;
            // println!("Checking binder = {}, end = {}", binder, end);
            let virt_page_num = VirtAddr(binder).into();
            if let Some (ppn_exists) = pt.translate(virt_page_num) {
                // println!("binder: {:#x}, virt_page_num.0: {:#x}, ppn_exists.bits: {:#x}", binder, virt_page_num.0, ppn_exists.bits);
                if ppn_exists.bits == 0 {
                    // println!("ppn is 0, Error!");
                    return -1;
                }
            }
            binder = binder + PAGE_SIZE;
        }
        let mut binder = _start;
        // Allocate
        while binder < ( end / PAGE_SIZE + 1 ) * PAGE_SIZE {
            // let _vpn = binder / PAGE_SIZE;
            let virt_page_num = VirtAddr(binder).into();
            PageTable::unmap(&mut pt, virt_page_num);
            binder = binder + PAGE_SIZE;
        }
        0
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

    // trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    let path = translated_str(new_task.get_user_token(), _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        new_task.exec(data);
    } else {
    }
    add_task(new_task);
    new_pid as isize
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    /* trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1 */
    if _prio < 2 {
        -1
    } else {
        let task = current_task().unwrap();
        // ---- access current PCB exclusively
        let mut inner = task.inner_exclusive_access();
        inner.prio = _prio;
        _prio
    }
}
