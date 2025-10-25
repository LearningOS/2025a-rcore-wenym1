use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}

fn request<S: ?Sized>(
    sync_var: &mut Vec<Option<(Arc<S>, usize, Vec<usize>, Vec<usize>)>>,
    var_id: usize,
    tid: usize,
) -> Arc<S> {
    let (var, _total, _, requests) = sync_var[var_id].as_mut().unwrap();
    requests.push(tid);
    Arc::clone(var)
}

fn acquired<S: ?Sized>(
    sync_var: &mut Vec<Option<(Arc<S>, usize, Vec<usize>, Vec<usize>)>>,
    var_id: usize,
    tid: usize,
) {
    let (_, _total, allocated, requests) = sync_var[var_id].as_mut().unwrap();
    let (id, _) = requests
        .iter()
        .enumerate()
        .find(|(_, request_tid)| **request_tid == tid)
        .unwrap();
    requests.remove(id);
    allocated.push(tid);
}

fn release<S: ?Sized>(
    sync_var: &mut Vec<Option<(Arc<S>, usize, Vec<usize>, Vec<usize>)>>,
    var_id: usize,
    tid: usize,
) {
    let (_, total, allocated, _) = sync_var[var_id].as_mut().unwrap();
    if let Some((id, _)) = allocated
        .iter()
        .enumerate()
        .find(|(_, allocated_tid)| **allocated_tid == tid)
    {
        allocated.remove(id);
    } else {
        *total += 1;
        // panic!("nothing to release {} {}", var_id, tid);
    }
}

#[derive(Debug)]
struct VecMap<V> {
    items: Vec<(usize, V)>,
}

impl<V> Default for VecMap<V> {
    fn default() -> Self {
        Self { items: vec![] }
    }
}

impl<V> VecMap<V> {
    fn get(&self, id: usize) -> Option<&V> {
        self.items
            .iter()
            .find(|(item_id, _)| *item_id == id)
            .map(|(_, value)| value)
    }

    fn get_mut(&mut self, id: usize) -> &mut V {
        &mut self
            .items
            .iter_mut()
            .find(|(item_id, _)| *item_id == id)
            .unwrap()
            .1
    }

    fn remove(&mut self, id: usize) -> V {
        let (id, _) = self
            .items
            .iter_mut()
            .enumerate()
            .find(|(_, (item_id, _))| *item_id == id)
            .unwrap();
        self.items.remove(id).1
    }

    fn get_mut_or_insert_default(&mut self, id: usize) -> &mut V
    where
        V: Default,
    {
        let idx = if let Some((idx, _)) = self
            .items
            .iter_mut()
            .enumerate()
            .find(|(_, (item_id, _))| *item_id == id)
        {
            idx
        } else {
            let idx = self.items.len();
            self.items.push((id, V::default()));
            idx
        };

        &mut self.items[idx].1
    }
}

fn reject_deadlock<S: ?Sized>(
    sync_var: &Vec<Option<(Arc<S>, usize, Vec<usize>, Vec<usize>)>>,
    var_id: usize,
    tid: usize,
) -> bool {
    // var_id -> available
    let mut works = VecMap::default();
    // tid -> var -> request
    let mut needs: VecMap<VecMap<usize>> = VecMap::default();
    // tid -> var -> allocated
    let mut allocation: VecMap<VecMap<usize>> = VecMap::default();
    // // println!("check reject deadlock {} {}", var_id, tid);
    *needs
        .get_mut_or_insert_default(tid)
        .get_mut_or_insert_default(var_id) = 1;
    for (var_id, var) in sync_var.iter().enumerate() {
        let Some((_, total, allocated, requests)) = var else {
            continue;
        };
        *works.get_mut_or_insert_default(var_id) = (*total as isize) - (allocated.len() as isize);
        for request_tid in requests {
            *needs
                .get_mut_or_insert_default(*request_tid)
                .get_mut_or_insert_default(var_id) += 1;
        }
        for allocated_tid in allocated {
            *allocation
                .get_mut_or_insert_default(*allocated_tid)
                .get_mut_or_insert_default(var_id) += 1;
            needs.get_mut_or_insert_default(*allocated_tid);
        }
    }
    // // println!("initialize {:#?} {:#?} {:#?}", works, needs, allocation);
    'outer: loop {
        if needs.items.is_empty() {
            break false;
        }
        for (tid, t_needs) in needs.items.iter() {
            if t_needs.items.iter().all(|(var_id, var_need)| {
                *var_need as isize <= *works.get(*var_id).expect("exists for var")
            }) {
                let tid = *tid;
                needs.remove(tid);
                if let Some(allocation) = allocation.get(tid) {
                    for (var_id, var_allocated) in &allocation.items {
                        *works.get_mut(*var_id) += *var_allocated as isize;
                    }
                }
                continue 'outer;
            }
        }
        // println!("remaining {:#?} {:#?} {:#?}", works, needs, allocation);
        break true;
    }
}

/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Arc<dyn Mutex> = if !blocking {
        Arc::new(MutexSpin::new())
    } else {
        Arc::new(MutexBlocking::new())
    };
    let mutex = Some((mutex, 1, vec![], vec![]));
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    if process_inner.enable_deadlock_detect
        && reject_deadlock(&process_inner.mutex_list, mutex_id, tid)
    {
        return -0xDEAD;
    }
    // // println!("request mutex {} {}", mutex_id, tid);
    let mutex = request(&mut process_inner.mutex_list, mutex_id, tid);
    drop(process_inner);
    drop(process);
    mutex.lock();
    // // println!("acquire mutex {} {}", mutex_id, tid);
    acquired(
        &mut current_process().inner_exclusive_access().mutex_list,
        mutex_id,
        tid,
    );
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(&process_inner.mutex_list[mutex_id].as_ref().unwrap().0);
    drop(process_inner);
    drop(process);
    mutex.unlock();
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    // // println!("release mutex {} {}", mutex_id, tid);
    release(
        &mut current_process().inner_exclusive_access().mutex_list,
        mutex_id,
        tid,
    );
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some((
            Arc::new(Semaphore::new(res_count)),
            res_count,
            vec![],
            vec![],
        ));
        id
    } else {
        process_inner.semaphore_list.push(Some((
            Arc::new(Semaphore::new(res_count)),
            res_count,
            vec![],
            vec![],
        )));
        process_inner.semaphore_list.len() - 1
    };
    // println!("create sem {}", id);
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(&process_inner.semaphore_list[sem_id].as_ref().unwrap().0);
    drop(process_inner);
    sem.up();
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    // println!("release sem {} {}", sem_id, tid);
    release(
        &mut current_process().inner_exclusive_access().semaphore_list,
        sem_id,
        tid,
    );
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    if process_inner.enable_deadlock_detect
        && reject_deadlock(&mut process_inner.semaphore_list, sem_id, tid)
    {
        return -0xDEAD;
    }

    // println!("request sem {} {}", sem_id, tid);
    let sem = request(&mut process_inner.semaphore_list, sem_id, tid);
    drop(process_inner);
    sem.down();
    // println!("acquire sem {} {}", sem_id, tid);
    acquired(
        &mut current_process().inner_exclusive_access().semaphore_list,
        sem_id,
        tid,
    );
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(&process_inner.mutex_list[mutex_id].as_ref().unwrap().0);
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect NOT IMPLEMENTED");
    match enabled {
        0 | 1 => {
            current_process()
                .inner_exclusive_access()
                .enable_deadlock_detect = enabled == 1;
            0
        }
        _ => -1,
    }
}
