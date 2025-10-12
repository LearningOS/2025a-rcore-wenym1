## 功能实现

在 `TaskControlBlock` 中加入字段 `syscall_cnt: [usize; 5]` 记录每个系统调用
的调用次数，并将数组元素初始化为0。提供函数 `syscall_trace_idx` 将 `syscall_id`
转化为系统调用在数组中的下标。在 `syscall` 函数入口处将当前系统调用的 `syscall_cnt`
+1。在处理 `sys_trace` 时将对应系统调用的 `syscall_cnt` 返回。

## 简答题
### 问题1
日志报错
```txt
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003a4, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
```
在 `rust_main` 中，`trap::init()` 将 `__alltraps` 注册到 `stvec` 寄存器中。
非法访问会触发注册的 `__alltraps` 并最终调用 `trap_handler`.

运行时使用的rustsbi版本为`0.2.0-alpha.2`

### 问题2
#### 1
刚进入 `__restore` 时，`sp` 表示调用 `__restore` 时内核态运行时的栈指针。
`__restore` 会分别在开始运行新的应用，以及在一个应用被中断挂起后恢复执行时调用。

#### 2
sstatus: 发生中断时用户态的CPU状态
sepc: 保存返回用户态后要执行的指令的地址
sscratch: 写入触发中断时的用户态的sp值

### 3
x2是sp寄存器，在sret后自动写入。x4是线程指针寄存器，在进程运行期间通常保持不变

#### 4
执行后sp指向用户态栈指针，sscratch指向内核态的栈指针.

#### 5
`sret` 发生状态切换。上下文的寄存器信息已经加载完成，用户态进入trap时的指令指针已经写入`sepc`
寄存器。`sret`后将降级并恢复用户态原来的状态并继续执行。

#### 6
执行后sscratch指向用户态栈指针，sp指向内核态的栈指针.

#### 7
由 `ecall` 主动触发，或者由用户态非法操作触发。

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

> 无

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

> 无

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

