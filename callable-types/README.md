## User-defined callable types

Various languages have ways of making user-defined objects callable with
function call syntax: C++'s [`operator ()`][cpp], Python's [`__call__`][python],
Swift's [`@dynamicCallable`][swift], Kotlin's [`invoke`][kotlin], PHP's
[`__invoke`][php], Scala's [`apply`][scala], etc.

[cpp]: https://en.cppreference.com/w/cpp/language/operators#Function_call_operator
[python]: https://docs.python.org/3/reference/datamodel.html#object.__call__
[swift]: https://docs.swift.org/swift-book/ReferenceManual/Attributes.html
[kotlin]: https://kotlinlang.org/docs/reference/operator-overloading.html#invoke
[php]: https://www.php.net/manual/en/language.oop5.magic.php#object.invoke
[scala]: https://scala-lang.org/files/archive/spec/2.12/06-expressions.html#function-applications

Something along these lines exists in Rust in the form of the [`std::ops::Fn`]
trait. When you write a closure expression, under the hood it becomes a struct
with some unique type that captures the necessary state from the closure's
environment and provides an implementation of this `Fn` trait to make it
callable. This isn't quite like the examples cited from other languages because
the trait can only be implemented by the compiler, not by the user for their own
data structures.

[`std::ops::Fn`]: https://doc.rust-lang.org/nightly/std/ops/trait.Fn.html

I was playing around with this functionality involving closures to stretch the
possibilities a bit. Mainly I wondered whether there is anything that can be
written in the gap in the code below to make our data structure work like a
callable function object *on a stable compiler* despite this not being a feature
of the language.

```rust
/// Function object that adds some number to its input.
struct Plus {
    n: u32,
}

impl Plus {
    fn call(&self, arg: u32) -> u32 {
        self.n + arg
    }
}

// [Something special here ...]

fn main() {
    let one_plus = Plus { n: 1 };
    let sum = one_plus(2);
    assert_eq!(sum, 1 + 2);
}
```

It turns out that yes, it is possible to make this work (with caveats).

<br>

### Background

We will use an interesting combination of `Deref`, closures, trait objects, and
unsafe code.

We will stick to functions with the signature `fn(&self, u32) -> u32` to get the
simplest thing working, but everything generalizes to other signatures.

To explain the relevance of `Deref`, observe that the function call operator
performs deref coercions to find a `Fn` impl. In the following code we write
`f(2)` to call an object `f` of type `&Callable`, which does not itself
implement the `Fn` trait. But `&Callable` dereferences to `&fn(u32) -> u32`
which does, so that is what gets called.

```rust
use std::ops::Deref;

struct Callable;

impl Deref for Callable {
    type Target = fn(u32) -> u32;

    fn deref(&self) -> &'static Self::Target {
        &(one_plus as fn(u32) -> u32)
    }
}

fn one_plus(arg: u32) -> u32 {
    1 + arg
}

fn main() {
    let f = &Callable;
    assert_eq!(f(2), 1 + 2);
}
```

<br>

### First attempt

The code under Background is syntactically on the right track because it enables
writing parentheses for function call notation on a value of user-defined type.
But since the thing being called in that code after deref coercion is just a
function pointer, the value of `self` (the object being invoked as a function)
is not accessible to the function body, which makes this severely limited in
usefulness.

What we want conceptually is this kind of thing:

```rust
impl Callable {
    fn call(&self, arg: u32) -> u32 {
        // Function body
    }
}

impl Deref for Callable {
    type Target = ???;

    fn deref(&self) -> &Self::Target {
        &|arg| self.call(arg)
    }
}
```

That is, the thing being called after deref coercion would be a closure that has
captured `self` and receives all the non-`self` args to set up a call to the
intended function body.

We can even spell out a type for `Target` that makes this look correctly typed.

```rust
impl Deref for Callable {
    type Target = dyn Fn(u32) -> u32;

    fn deref(&self) -> &Self::Target {
        &|arg| self.call(arg)
    }
}
```

The borrow checker explains (not that clearly in this case) that this
implementation would not be sound. The reference being returned by `deref` is
dangling because it refers to a closure object on the stack frame of the `deref`
call that is destroyed during the return.

```console
error[E0495]: cannot infer an appropriate lifetime due to conflicting requirements
  --> src/main.rs:15:10
   |
15 |         &|arg| self.call(arg)
   |          ^^^^^^^^^^^^^^^^^^^^
   |
note: first, the lifetime cannot outlive the anonymous lifetime #1 defined on the method body at 14:5...
  --> src/main.rs:14:5
   |
14 | /     fn deref(&self) -> &Self::Target {
15 | |         &|arg| self.call(arg)
16 | |     }
   | |_____^
   = note: ...so that the types are compatible:
           expected &&Callable
              found &&Callable
   = note: but, the lifetime must be valid for the static lifetime...
   = note: ...so that the expression is assignable:
           expected &(dyn std::ops::Fn(u32) -> u32 + 'static)
              found &dyn std::ops::Fn(u32) -> u32
```

To see it more clearly, this closure would have desugared to something like the
following:

```rust
impl Deref for Callable {
    type Target = dyn Fn(u32) -> u32;

    fn deref(&self) -> &Self::Target {
        // Generated by the compiler as the memory representation
        // of `|arg| self.call(arg)`.
        struct GeneratedClosure<'a> {
            self_: &'a Callable,
        }

        // Also generated by the compiler.
        impl<'a> Fn(u32) -> u32 for GeneratedClosure<'a> {
            fn call(&self, arg: u32) -> u32 {
                let self_ = self.self_;

                // Body of `|arg| self.call(arg)`.
                self_.call(arg)
            }
        }

        // Expanded view of `&|arg| self.call(arg)`.
        let generated_closure = GeneratedClosure { self_: self };
        let reference_to_closure: &GeneratedClosure = &generated_closure;
        let reference_to_trait_object = reference_to_closure as &dyn Fn(u32) -> u32;
        reference_to_trait_object
    }
}
```

<br>

### Second attempt

If we temporarily conflate the types `GeneratedClosure` and `&Callable`, notice
how in the desugared code from the first attempt we have `deref` returning
`&&Callable` (as a reference to trait object) and `GeneratedClosure::call`
accepting `&&Callable` as its first argument. The inner reference lives long
enough to match deref's signature but the outer reference does not; the outer
reference points to the inner reference which exists on `deref`'s stack frame
and goes out of scope.

What we would love to trick the compiler into doing is something more like:

```rust
impl Deref for Callable {
    type Target = dyn Fn(u32) -> u32;

    fn deref(&self) -> &Self::Target {
        // Generated by the compiler (???)
        #[repr(transparent)]
        struct GeneratedClosure {
            self_: Callable,
        }

        // Also generated by the compiler (???)
        impl Fn(u32) -> u32 for GeneratedClosure {
            fn call(&self, arg: u32) -> u32 {
                let self_ = &self.self_;

                // Body of the closure we would write.
                self_.call(arg)
            }
        }

        let reference_to_closure = &GeneratedClosure { self_: *self };
        let reference_to_trait_object = reference_to_closure as &dyn Fn(u32) -> u32;
        reference_to_trait_object
    }
}
```

Here instead we have `deref` returning `&Callable` (as a reference to trait
object) and `GeneratedClosure::call` accepting `&Callable`. The conversion from
`&Callable` to `&GeneratedClosure` is sound as long as `Callable` and
`GeneratedClosure` have the same memory representation, which would be
guaranteed by `#[repr(transparent)]`. That conversion results in a reference
pointing to the caller's `Callable` rather than to anything on `deref`'s stack
frame, so it lives long enough that this would be a safe and working
implementation of the intended functionality.

Let's think about what closure we would need to write in order for the compiler
to come up with the above data structure and `Fn` trait impl.

We know it would need to capture a value of type `Callable` by value. This
begins to sound problematic because there would never exist an owned value of
type `Callable` accessible to the `Deref` impl, only as a borrowed `&Callable`.

But an imaginary uninitialized `Callable` gets the job done:

```rust
let uninit_callable: Callable = unsafe { mem::uninitialized() };
let uninit_closure = move |arg: u32| Callable::call(&uninit_callable, arg);
mem::forget(uninit_closure);
```

This code makes an uninitialized owned `Callable`, moves ownership of it into a
closure that captures a `Callable` by value and nothing else, and then prevents
a `Drop` call on the closure because we must not drop its uninitialized
contents. At runtime this would all be noop but it gets the compiler to generate
the right data structure and `Fn` trait impl shown above.

The remaining part is to turn `self` into a trait object based on this `Fn`
impl, the equivalent of `&GeneratedClosure { self_: *self } as &dyn Fn(u32) ->
u32`.

Ordinarily we would reach for a `mem::transmute::<&Callable,
&GeneratedClosure>(self)` or `&*(self as *const Callable as *const
GeneratedClosure)`, but in this case that won't work because the closure's real
type is generated and does not have a name that we can refer to. A different
technique is needed:

```rust
fn second<'a, T>(_a: &T, b: &'a T) -> &'a T {
    b
}
let reference_to_closure = second(&uninit_closure, unsafe { mem::transmute(self) });
```

This uses generic type inference to deduce the return type of the transmute as
identical to a reference to the closure's type, whatever that might be.

At this point we have a closure to make into a trait object.

```rust
let reference_to_trait_object = reference_to_closure as &dyn Fn(u32) -> u32;
```

The impl all at once looks like:

```rust
impl Deref for Callable {
    type Target = dyn Fn(u32) -> u32;

    fn deref(&self) -> &Self::Target {
        let uninit_callable: Self = unsafe { mem::uninitialized() };
        let uninit_closure = move |arg: u32| Self::call(&uninit_callable, arg);
        fn second<'a, T>(_a: &T, b: &'a T) -> &'a T {
            b
        }
        let reference_to_closure = second(&uninit_closure, unsafe { mem::transmute(self) });
        mem::forget(uninit_closure);
        let reference_to_trait_object = reference_to_closure as &dyn Fn(u32) -> u32;
        reference_to_trait_object
    }
}
```

<br>

### Third attempt

I called out `#[repr(transparent)]` earlier on, but then didn't bring it up
again in the context of the closure-based implementation. We have written a
closure that captures a type `Callable` by value so it makes sense why it would
be represented like `struct GeneratedClosure { captured: Callable }` but:

- it is not a guarantee made by the language that a closure capturing `Callable`
  by value is represented in memory the same as `struct { Callable }`;

- nor is it a guarantee that `struct { Callable }` would be represented the same
  as `Callable`.

So this is the big caveat; don't count on this to work now or continue working
in the future. Nothing on this page is a robust solution, only interesting. For
now I think this is the closest we get, by adding an assertion as a basic smoke
test that the closure matches the expected size:

```rust
use std::mem;
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
        let uninit_callable: Self = unsafe { mem::uninitialized() };
        let uninit_closure = move |arg: u32| Self::call(&uninit_callable, arg);
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
}
```

<br>

### Fourth attempt

There is one remaining problem to sort out. The following line from the third
attempt may contain undefined behavior:

```rust
let uninit_callable: Self = unsafe { mem::uninitialized() };
```

Usually the most common way that creating an uninitialized value of an unknown
type in generic code causes undefined behavior is if an expression like
`mem::uninitialized::<T>()` might be instantiated with a choice of `T` that is
uninhabited, such as the `!` type. When that happens, the compiler is free to
turn the `mem::uninitialized` call into [`unreachable_unchecked`] and plummet
off the end of your function, even though you intended for this line to be a
noop.

[`unreachable_unchecked`]: https://doc.rust-lang.org/std/hint/fn.unreachable_unchecked.html

As used here, that's not a concern -- we know `Self` is inhabited at runtime
because there exists a `&Self` in scope that was passed in by the caller. If
`Self` were uninhabited, it would be impossible for the caller to have an
instance of `Self` on which to borrow (`&self`) and call `deref`.

Instead we need to worry about the second most common way that creating
uninitialized values of an unknown type causes undefined behavior, and that's if
the uninitialized type has nontrivial validity invariants. In our case if the
memory representation of `Self` contains a bool, char, `&`, `&mut`, Box,
NonZero, or any other type where not all possible values are valid, then
`mem::uninitialized::<Self>()` is immediate UB.

The correct way to manipulate uninitialized memory of generic type is through
[`MaybeUninit`].

[`MaybeUninit`]: https://doc.rust-lang.org/std/mem/union.MaybeUninit.html

```rust
let uninit_callable = MaybeUninit::<Self>::uninit();
let uninit_closure = move |arg: u32| Self::call(
    unsafe { &*uninit_callable.as_ptr() },
    arg,
);
```

The final expanded code all together is:

```rust
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
        let uninit_closure = move |arg: u32| Self::call(
            unsafe { &*uninit_callable.as_ptr() },
            arg,
        );
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
}
```

<br>

### Implementation

Packaging this up into a macro is the easy part. We would most likely want an
attribute macro on an impl block that turns the block's one method into the fake
`Fn` impl.

```rust
/// Function object that adds some number to its input.
struct Plus {
    n: u32,
}

#[hackfn]
impl Plus {
    fn call(&self, arg: u32) -> u32 {
        self.n + arg
    }
}

fn main() {
    let one_plus = Plus { n: 1 };
    let sum = one_plus(2);
    assert_eq!(sum, 1 + 2);
}
```

<br>

End note: I feel that the technique of returning trait objects from
`&`-returning trait methods like `Deref`, `Index`, `Borrow` etc is underexplored
and there are major impactful applications waiting to be discovered in that
area. [This StackOverflow answer][hashmap] demonstrates one amazing example in
the context of *How to implement HashMap with two keys?*. A more basic one is
the [slice of a multidimensional array][refcast] example from RefCast; this
involves a dynamically sized slice rather than a trait object but the underlying
idea is similar. I think that these two and the case study are scratching the
surface of something bigger with exciting applications. Note that those two
links are all safe code; unsafe is not inherent to this technique.

[hashmap]: https://stackoverflow.com/a/45795699/6086311
[refcast]: https://github.com/dtolnay/ref-cast#realistic-example
