use std::mem;

pub struct S(i32);
pub type Arg1<'a> = &'a i32;
pub type Arg2 = i32;
pub type Ret<'a> = (&'a mut i32, i32);

impl S {
    pub fn original_f(&mut self, a: Arg1, b: Arg2) -> Ret {
        (&mut self.0, a + b)
    }

    pub fn generated_f(&mut self, a: Arg1, b: Arg2) -> Ret {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                // Do the thing
            }
        }
        let guard = Guard;

        let value = (move || {
            let _self = self;
            let a = a;
            let b = b;

            // Original function body, with self replaced by _self
            // except in nested impls:

            (&mut _self.0, a + b)
        })();

        mem::forget(guard);
        value
    }
}

fn main() {}
