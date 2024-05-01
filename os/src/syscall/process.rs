//! Process management syscalls
use crate::{
    config::MAX_SYSCALL_NUM,
    task::{count_syscall, exit_current_and_run_next, get_current_process_run_time, get_syscall_times, suspend_current_and_run_next, TaskStatus},
    timer::get_time_us,
};



/// Time value structure
#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    /// second
    pub sec: usize,
    /// microsecond
    pub usec: usize,
}

/// Task information
#[derive(Copy, Clone)]
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
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    let status = TaskStatus::Running;
    let mut syscall_times = [0u32; MAX_SYSCALL_NUM];
    get_syscall_times(&mut syscall_times);
    unsafe {
        *_ti = TaskInfo {
            status,
            syscall_times,
            time: get_current_process_run_time(),
        };
    }
    0
}
