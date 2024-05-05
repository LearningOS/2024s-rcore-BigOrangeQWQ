//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::config::{MAX_SYSCALL_NUM, PAGE_SIZE};
use crate::loader::{get_app_data, get_num_app};
use crate::mm::{MapPermission, VirtAddr};
use crate::sync::UPSafeCell;
use crate::timer::get_time_ms;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,

}

/// Task information 
#[derive(Copy, Clone)]
pub struct TaskInfo {
    /// last start time
    start_time: usize,
    /// syscall times
    syscall: [u32; MAX_SYSCALL_NUM]
}


impl TaskInfo {
    /// Create a new TaskInfo
    pub fn new() -> Self {
        TaskInfo {
            start_time: 0, 
            syscall: [0; MAX_SYSCALL_NUM],
        }
    }

    /// Update the start time of TaskInfo
    pub fn update_time(&mut self) {
        if self.start_time == 0 {
            self.start_time = get_time_ms();
        }
    }

    /// Get the running time of TaskInfo
    pub fn get_time(&mut self) -> usize {
        get_time_ms() - self.start_time
    }
}


/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
    /// task information
    infos: [TaskInfo; MAX_SYSCALL_NUM],
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                    infos: [TaskInfo::new(); MAX_SYSCALL_NUM],
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        inner.infos[0].update_time();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.infos[next].update_time();
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    fn map_memory(&self, start: usize, len: usize, perm: usize) -> isize {
        if start % PAGE_SIZE != 0 {
            return -1;
        }
        if perm & !0x7 != 0 || perm & 0x7 == 0 {
            return -1;
        }

        let start_va = VirtAddr::from(start);
        let end_va = VirtAddr::from(start + len);


        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;

        let memory_set = &mut inner.tasks[current].memory_set;

        if memory_set.check_framed_area(start_va, end_va) {
            return -1;
        }

        memory_set.insert_framed_area(start_va,
            end_va, MapPermission::from_bits((perm << 1) as u8).unwrap() | MapPermission::U
        );
        
        0
    }

    fn unmap_memory(&self, start: usize, len: usize) -> isize {
        if start % PAGE_SIZE != 0 {
            return -1;
        }
    
        let start_va = VirtAddr::from(start);
        let end_va = VirtAddr::from(start + len);

        // println!("start_va = {:?}, end_va = {:?}", start_va, end_va);

        if !start_va.aligned() || !end_va.aligned() {
            return -1;
        }

        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;

        let memory_set = &mut inner.tasks[current].memory_set;

        memory_set.remove_framed_area(start_va, end_va);
        0
    }

    fn count_sys_call(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.infos[current].syscall[syscall_id] += 1;
        drop(inner);
    }

    /// Get syscall times of current `Running` task.
    fn get_syscall_times(&self, syscall_num: &mut [u32; MAX_SYSCALL_NUM]) {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        syscall_num.copy_from_slice(&inner.infos[current].syscall);
        drop(inner);
    }

    fn get_run_time(&self) -> usize {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let time = inner.infos[current].get_time();
        drop(inner);
        time
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// Count the number of syscalls of current `Running` task.
pub fn count_syscall(syscall_id: usize) {
    TASK_MANAGER.count_sys_call(syscall_id);
}

/// Get syscall times of current `Running` task.
pub fn get_syscall_times(syscall_num: &mut [u32; MAX_SYSCALL_NUM]) {
    TASK_MANAGER.get_syscall_times(syscall_num);
}

/// Get running time of current `Running` task.
pub fn get_current_process_run_time() -> usize {
    TASK_MANAGER.get_run_time()
}

/// Map memory for current `Running` task.
pub fn current_process_map_memory(start: usize, len: usize, perm: usize) -> isize {
    TASK_MANAGER.map_memory(start, len, perm)
}

/// Unmap memory for current `Running` task.
pub fn current_process_unmap_memory(start: usize, len: usize) -> isize {
    TASK_MANAGER.unmap_memory(start, len)
}