//! Process management syscalls

use crate::{
    config::MAX_SYSCALL_NUM, mm::translated_struct, task::{
        change_program_brk, current_process_map_memory, current_process_unmap_memory, current_user_token, exit_current_and_run_next, get_current_process_run_time, get_syscall_times, suspend_current_and_run_next, TaskStatus
    }
};

#[repr(C)]
#[derive(Debug)]
#[allow(missing_docs)]
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
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    //_ts 指针指向的内存区域是不知道的，我们可以通过unsafe获取指针，
        //然后根据sys_write的例子，即使用translated写入数据
    let us = crate::timer::get_time_us();
    let ts = translated_struct(current_user_token(),_ts);
    
    ts.sec = us / 1_000_000;
    ts.usec = us % 1_000_000;
    // println!("OMG::{:?}",us);
    // *ts = TimeVal {
    //     sec: us / 1_000_000,
    //     usec: us % 1_000_000,
    // };
    
    // let page_table = PageTable::from_token(current_user_token());
    // let start_va = VirtAddr::from(_ts as usize);
    // let vpn = start_va.floor();
    // let ppn = page_table.translate(vpn).unwrap().ppn();
    // println!("vpn:{:?}",PhysAddr::from(ppn).get_mut::<TimeVal>());
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    let status = TaskStatus::Running;
    let mut syscall_times = [0u32; MAX_SYSCALL_NUM];
    get_syscall_times(&mut syscall_times);
    let ti = translated_struct(current_user_token(), _ti);

    *ti = TaskInfo {
        status,
        syscall_times,
        time: get_current_process_run_time(),
    };
    
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap");
    current_process_map_memory(_start, _len, _port)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap");
    println!("munmap start: {:x}, len: {:x}", _start, _len);
    current_process_unmap_memory(_start, _len)
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

