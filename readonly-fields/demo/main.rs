pub use oqueue::Task;

mod oqueue {
    use core::ops::Deref;

    #[derive(Default)]
    #[repr(C)]
    pub struct Task {
        #[cfg(oqueue_docs)]
        pub index: usize,

        #[cfg(not(oqueue_docs))]
        index: usize,

        // Other private fields:
        q: usize,
    }

    #[doc(hidden)]
    #[repr(C)]
    pub struct ReadOnlyTask {
        pub index: usize,

        // The same private fields:
        q: usize,
    }

    #[doc(hidden)]
    impl Deref for Task {
        type Target = ReadOnlyTask;

        fn deref(&self) -> &Self::Target {
            unsafe { &*(self as *const Self as *const Self::Target) }
        }
    }

    #[allow(dead_code)]
    pub fn from_within_module(task: &mut Task) {
        task.index += 1;
    }
}

fn from_outside_module(task: &mut Task) {
    task.index += 1; // cannot assign
}

fn main() {
    let mut task = Task::default();
    oqueue::from_within_module(&mut task);
    from_outside_module(&mut task);
}
