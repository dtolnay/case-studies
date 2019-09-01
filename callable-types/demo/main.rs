use std::mem::{self, MaybeUninit};
use std::ops::Deref;

/// Function object that adds some number to its input.
struct Plus {
    n: u32,
}

impl Plus {
    fn call(&self, arg: u32) -> u32 {
        self.n + arg
    }
}

impl Deref for Plus {
    type Target = dyn Fn(u32) -> u32;

    fn deref(&self) -> &Self::Target {
        let uninit_callable = MaybeUninit::<Self>::uninit();
        let uninit_closure = move |arg: u32| Self::call(unsafe { &*uninit_callable.as_ptr() }, arg);
        let size_of_closure = mem::size_of_val(&uninit_closure);
        fn second<'a, T>(_a: &T, b: &'a T) -> &'a T {
            b
        }
        let reference_to_closure = second(&uninit_closure, unsafe { mem::transmute(self) });
        mem::forget(uninit_closure);
        assert_eq!(size_of_closure, mem::size_of::<Self>());
        let reference_to_trait_object = reference_to_closure as &dyn Fn(u32) -> u32;
        reference_to_trait_object
    }
}

fn main() {
    let one_plus = Plus { n: 1 };
    let sum = one_plus(2);
    assert_eq!(sum, 1 + 2);
    dbg!(one_plus(2));
}
