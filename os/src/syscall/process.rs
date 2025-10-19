//! Process management syscalls

use crate::config::PAGE_SIZE;
use crate::mm::{frame_alloc, PTEFlags, PageTable, PageTableEntry, VirtAddr, VirtPageNum};
use crate::syscall::syscall_trace_idx;
use crate::task::{
    change_program_brk, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next, TASK_MANAGER,
};
use crate::timer::get_time_us;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::size_of;
use core::ptr::addr_of_mut;

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

fn translate_user_ptr(addr: VirtAddr, entry: PageTableEntry) -> &'static mut u8 {
    &mut entry.ppn().get_bytes_array()[addr.page_offset()]
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let page_table = PageTable::from_token(current_user_token());
    fn set_usize(page_table: &PageTable, ptr: *mut usize, value: usize) {
        let target_ptr = ptr as *mut u8;
        let value_ptr = &value as *const usize as *const [u8; size_of::<usize>()];
        unsafe {
            for i in 0..size_of::<usize>() {
                let target_addr = VirtAddr::from(target_ptr.add(i) as *const u8 as usize);
                *translate_user_ptr(
                    target_addr,
                    page_table.translate(target_addr.floor()).unwrap(),
                ) = (*value_ptr)[i];
            }
        }
    }
    unsafe {
        set_usize(&page_table, addr_of_mut!((*ts).sec), us / 1_000_000);
        set_usize(&page_table, addr_of_mut!((*ts).usec), us % 1_000_000);
    }
    0
}

/// sys_trace
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => {
            let Some(addr) = VirtAddr::try_from(id) else {
                return -1;
            };
            let page_table = PageTable::from_token(current_user_token());
            if let Some(pte) = page_table.translate(addr.floor())
                && pte.is_valid()
                && pte.readable()
            {
                let ret = *(translate_user_ptr(addr, pte)) as isize;
                ret
            } else {
                -1
            }
        },
        1 => {
            let Some(addr) = VirtAddr::try_from(id) else {
                return -1;
            };
            let page_table = PageTable::from_token(current_user_token());
            if let Some(pte) = page_table.translate(addr.floor())
                && pte.is_valid()
                && pte.writable()
            {
                *(translate_user_ptr(addr, pte)) = data as _;
                0
            } else {
                -1
            }
        },
        2 => {
            let trace_idx = syscall_trace_idx(id);
            TASK_MANAGER.get_current_task_syscall_cnt(trace_idx) as _
        }
        _ => -1,
    }
}

fn get_pages(start: usize, len: usize) -> Vec<VirtPageNum> {
    let mut pages = vec![];
    let start_addr = VirtAddr::from(start);
    let end = start + len;
    let mut page_num = start_addr.floor();
    while VirtAddr::from(page_num).0 < end {
        pages.push(page_num);
        page_num.0 += 1;
    }
    pages
}

/// sys_mmap
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if prot & !0x7 != 0 || prot & 0x7 == 0 {
        return -1;
    }
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    let pages = get_pages(start, len);
    let mut page_table = PageTable::from_token(current_user_token());
    for page_num in &pages {
        if let Some(pte) = page_table.translate(*page_num)
            && pte.is_valid()
        {
            return -1;
        }
    }
    let mut pte_flag = PTEFlags::empty();
    pte_flag.set(PTEFlags::U, true);
    if prot & 0x1 != 0 {
        pte_flag.set(PTEFlags::R, true);
    }
    if prot & 0x2 != 0 {
        pte_flag.set(PTEFlags::W, true);
    }
    if prot & 0x4 != 0 {
        pte_flag.set(PTEFlags::X, true);
    }
    for i in 0..pages.len() {
        let Some(page) = frame_alloc() else {
            for i in 0..i {
                page_table.unmap(pages[i]);
            }
            return -1;
        };
        let page_num = pages[i];
        page_table.map(page_num, page.ppn, pte_flag);
    }
    0
}

/// sys_munmap
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    let pages = get_pages(start, len);
    let mut page_table = PageTable::from_token(current_user_token());
    for page in &pages {
        if let Some(pte) = page_table.translate(*page)
            && pte.is_valid()
        {
        } else {
            return -1;
        }
    }
    for page in pages {
        page_table.unmap(page);
    }
    0
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
