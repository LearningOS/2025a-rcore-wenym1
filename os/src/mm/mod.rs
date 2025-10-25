//! Memory management implementation
//!
//! SV39 page-based virtual-memory architecture for RV64 systems, and
//! everything about memory management, like frame allocator, page table,
//! map area and memory set, is implemented here.
//!
//! Every task or process has a memory_set to control its virtual memory.

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

use address::VPNRange;
pub use address::{PhysAddr, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use core::mem::size_of;
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
pub use memory_set::remap_test;
pub use memory_set::{kernel_token, MapPermission, MemorySet, KERNEL_SPACE};
use page_table::PTEFlags;
pub use page_table::{
    translated_byte_buffer, translated_ref, translated_refmut, translated_str, PageTable,
    PageTableEntry, UserBuffer, UserBufferIterator,
};

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}

fn translate_user_ptr(addr: VirtAddr, entry: PageTableEntry) -> &'static mut u8 {
    &mut entry.ppn().get_bytes_array()[addr.page_offset()]
}

/// set value in user space
pub fn set_value<T: Sized>(memory_set: &MemorySet, ptr: &mut T, value: T) {
    let target_ptr = ptr as *mut T as *mut u8;
    let value_ptr = &value as *const T as *const u8;
    unsafe {
        for i in 0..size_of::<T>() {
            let target_addr = VirtAddr::from(target_ptr.add(i) as *const u8 as usize);
            *translate_user_ptr(
                target_addr,
                memory_set.translate(target_addr.floor()).unwrap(),
            ) = *value_ptr.add(i);
        }
    }
}
