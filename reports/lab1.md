## 功能实现

在 `TaskControlBlock` 中加入字段 `syscall_cnt: [usize; 5]` 记录每个系统调用
的调用次数，并将数组元素初始化为0。提供函数 `syscall_trace_idx` 将 `syscall_id`
转化为系统调用在数组中的下标。在 `syscall` 函数入口处将当前系统调用的 `syscall_cnt`
+1。在处理 `sys_trace` 时将对应系统调用的 `syscall_cnt` 返回。

## 简答题
### 问题1
