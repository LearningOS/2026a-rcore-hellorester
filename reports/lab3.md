# 第五章实验报告：进程管理与 Stride 调度

## 一、功能总结

迁移 sys_get_time、mmap、munmap，适配新的进程结构；实现 spawn，直接加载指定程序创建子进程并建立父子关系；增加进程优先级与 stride 字段，实现优先级设置和 Stride 调度，按优先级分配调度机会，并处理计数回绕。

## 二、实现说明

### 1. 迁移内存相关系统调用

保留第四章时间缓冲区的跨页复制逻辑。mmap、munmap 通过 current_task 获取当前进程，再借用内部数据访问 memory_set，复用此前实现的区域映射、合法性检查和部分解除映射功能。本章按要求不再维护 sys_trace。

### 2. spawn 创建进程

将用户程序名转换为内核字符串，从框架预置的程序列表查找 ELF 数据；找不到程序时返回 -1。通过 TaskControlBlock::new 创建新的地址空间、进程编号、内核栈和初始上下文。子进程以弱引用记录父进程，父进程的 children 持有子进程的强引用，随后将子进程加入就绪队列并返回其 PID。

spawn 直接加载目标程序，不先复制父进程地址空间。父进程能够通过框架提供的 waitpid 等待和回收子进程。

### 3. 优先级与 Stride 调度

TaskControlBlockInner 新增 priority 和 u64 类型的 stride。new 和 fork 中均初始化为 priority=16、stride=0。sys_set_priority 拒绝小于 2 的参数，合法时更新当前进程优先级并返回该值。

调度器扫描就绪队列，取出 stride 最小的进程。选中后计算 pass=max(BIG_STRIDE/priority,1)，其中 BIG_STRIDE=2^32，并将 pass 累加到 stride。较高优先级的进程增长较慢，持续就绪时能获得更多调度机会。保留 VecDeque 实现，单次选择为 O(n)，足以满足当前实验规模。

累加使用 wrapping_add，比较使用 wrapping_sub 后转换为 i64 的有符号差值。此比较要求参与比较的逻辑计数距离小于 u64 模数的一半，并不是对任意两组 u64 值都成立的全序。极大优先级下步长被限制为 1，会产生整数粒度误差；新进程统一从零开始，也没有实现长期运行时的新任务公平性校正。

## 三、实验环境与验证

环境为 Ubuntu 22.04（WSL2），使用 nightly-2024-05-02、QEMU 8.2.2 和仓库附带的 RustSBI 0.4.1。

在仓库根目录执行 make -C os run BASE=2，启动后在内核终端运行 ch5_usertest。测试输出显示 ch5_spawn0、ch5_spawn1、ch5_setprio 和 ch5_stride 均以退出码 0 结束，最终输出 ch5 Usertests passed!。

此次 Stride 测试截图中，优先级为 5 至 10 的六个进程，其运行量与优先级的比值约为 353933 至 362320，最大值与最小值之比约为 1.024，小于测例要求的 1.5。该数据来自本次运行，不代表所有负载下的通用性能结论。

官方本地评测结果为 **15/15**，退出码为 **0**，lab1、lab2、lab3 报告文件检查全部通过。使用与仓库 CI 一致的 Checker 7d61ec55b58eed6ca7052917846c1b87af34563a 和 Test a0593662ad55d670ba8c27ce1763347cd0dd552f，在临时副本中完成评测。GitHub Actions 和成绩上传结果以提交后的线上运行记录为准。

## 四、问答作业

### 1. stride 溢出后是否仍会选择 p1？

不会。以 8 位无符号整数为例，p1.stride=255、p2.stride=250，二者步长均为 10。p2 运行一次后，逻辑值为 260，存入 8 位整数会回绕为 4。普通无符号比较得到 4<255，因此错误地再次选择 p2。实际应按逻辑值 255<260 选择 p1。

### 2. 为什么优先级至少为 2 时，差值不超过 BigStride/2？

在不溢出、进程集合固定且各进程初始 stride 相同的条件下，priority>=2 使每次增加的 pass 不超过 H=BigStride/2。

初始最大差值为 0。假设某轮开始时最小值为 m，其他进程均在 [m,m+H] 内；本轮只给最小者增加至多 H，因此更新后的全部值仍在该区间内，新最大值与最小值之差仍不超过 H。由归纳可得该界限。

这一论证不能直接推广到任意动态加入的新进程：若新进程总从 0 开始，而其他进程已运行很久，需要额外处理初始计数及公平性。本实验按指导要求采用初始值 0。

### 3. 如何设计能处理回绕的比较器？

设整数模数为 M，若两计数的真实距离严格小于 M/2，则可根据回绕差值的符号判断先后。例如 u64 中，a.wrapping_sub(b) 转换为 i64 后为负，表示在该前提下 a 应先于 b。

Rust 的 BinaryHeap 默认弹出最大元素，而调度需要取逻辑最小值，因此比较器还需反转次序。示意实现如下：

~~~rust
use core::cmp::Ordering;

#[derive(Eq, PartialEq)]
struct Stride(u64);

impl Ord for Stride {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.0 == other.0 {
            Ordering::Equal
        } else if (self.0.wrapping_sub(other.0) as i64) < 0 {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    }
}

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
~~~

该比较器仅适用于题设的有界距离场景，不能作为任意 u64 数据的通用排序器。本次内核采用线性扫描，不使用 BinaryHeap，因此扫描比较无需反转。

## 五、荣誉准则与参考说明

本实验基于课程提供的框架完成。开发中使用 Codex 辅助理解进程关系与调度原理、提供实现示例、迁移旧功能、排查编译问题和整理报告。spawn、优先级字段及设置、Stride 调度与回绕比较参考了 Codex 提供的代码；mmap、munmap 的迁移以及 fetch_task 恢复由 Codex 协助完成。本报告由 Codex 辅助整理，对这些辅助来源予以明确披露，不作完全独立完成的声明。

参考资料：

- [rCore-Tutorial-Guide 第五章练习](https://learningos.cn/rCore-Tutorial-Guide/chapter5/4exercise.html)
- 本期作业仓库的进程、调度、内存管理和系统调用源码。
- 官方 rCore-Tutorial-Test 与 rCore-Tutorial-Checker。

本次测试和报告仅描述已实现并验证的基础实验，不声称已实现写时复制、多核调度或其他选做功能。
