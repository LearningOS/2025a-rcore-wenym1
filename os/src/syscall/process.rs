//! Process management syscalls
use crate::config::PAGE_SIZE;
use crate::mm::{MapPermission, MemorySet, PageTableEntry, VirtAddr};
use crate::timer::get_time_us;
use crate::{
    loader::get_app_data_by_name,
    mm::{translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
};
use alloc::sync::Arc;
use core::mem::size_of;
use core::ptr::addr_of_mut;

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
    trace!(
        "kernel::pid[{}] sys_waitpid [{}]",
        current_task().unwrap().pid.0,
        pid
    );
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

fn translate_user_ptr(addr: VirtAddr, entry: PageTableEntry) -> &'static mut u8 {
    &mut entry.ppn().get_bytes_array()[addr.page_offset()]
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel:pid[{}]", current_task().unwrap().pid.0);
    let us = get_time_us();
    let task = current_task().unwrap();
    fn set_usize(memory_set: &MemorySet, ptr: *mut usize, value: usize) {
        let target_ptr = ptr as *mut u8;
        let value_ptr = &value as *const usize as *const [u8; size_of::<usize>()];
        unsafe {
            for i in 0..size_of::<usize>() {
                let target_addr = VirtAddr::from(target_ptr.add(i) as *const u8 as usize);
                *translate_user_ptr(
                    target_addr,
                    memory_set.translate(target_addr.floor()).unwrap(),
                ) = (*value_ptr)[i];
            }
        }
    }
    let task = task.inner_exclusive_access();
    unsafe {
        set_usize(&task.memory_set, addr_of_mut!((*ts).sec), us / 1_000_000);
        set_usize(&task.memory_set, addr_of_mut!((*ts).usec), us % 1_000_000);
    }
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if prot & !0x7 != 0 || prot & 0x7 == 0 {
        return -1;
    }
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    let start_addr = VirtAddr::from(start);
    let end = start + len;
    let task = current_task().unwrap();
    let mut task = task.inner_exclusive_access();
    {
        let mut page_num = start_addr.floor();
        while VirtAddr::from(page_num).0 < end {
            if let Some(pte) = task.memory_set.translate(page_num)
                && pte.is_valid()
            {
                return -1;
            }
            page_num.0 += 1;
        }
    }
    let end_addr = VirtAddr::from(end);
    let mut permission = MapPermission::empty();
    permission.set(MapPermission::U, true);
    if prot & 0x1 != 0 {
        permission.set(MapPermission::R, true);
    }
    if prot & 0x2 != 0 {
        permission.set(MapPermission::W, true);
    }
    if prot & 0x4 != 0 {
        permission.set(MapPermission::X, true);
    }
    task.memory_set
    .insert_framed_area(start_addr, end_addr, permission);
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_munmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    if current_task()
        .unwrap()
        .inner_exclusive_access()
        .memory_set
        .remove_frame_area(start_va, end_va)
    {
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
pub fn sys_spawn(path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let current_task = current_task().unwrap();
        let new_task = current_task.spawn(data);
        let new_pid = new_task.pid.0 as _;
        add_task(new_task);
        new_pid
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}
