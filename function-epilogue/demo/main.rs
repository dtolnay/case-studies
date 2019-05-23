use std::mem;

pub struct S(i32);
pub type Arg1 = i32;
pub type Arg2 = i32;
pub type Ret<'a> = (&'a i32, i32);

impl S {
    pub fn f(&self, a: Arg1, b: Arg2) -> Ret {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                // Do the thing
            }
        }
        let guard = Guard;

        let original_f = |_self: &Self, a: Arg1, b: Arg2| -> Ret {
            // Original function body, with self replaced by _self
            // except in nested impls:

            (&_self.0, a + b)
        } as fn(&Self, Arg1, Arg2) -> Ret;

        let value = original_f(self, a, b);

        mem::forget(guard);
        value
    }
}

fn main() {}
