## Read-only fields of mutable struct

In [`oqueue`] I wanted to expose a field of one of the structs in the API, but
not allow it to be mutated even if the caller has &amp;mut access to the
surrounding struct.

[`oqueue`]: https://github.com/dtolnay/oqueue

<br>

### Rejected approaches

<kbd>**Public field.**</kbd> The field cannot be `pub` because mutating it
directly would enable the caller to violate invariants of the API.

```rust
// Bad: caller can mutate, task.index += 1

pub struct Task {
    pub index: usize,
    // other private fields
}
```

<kbd>**Private field, public getter.**</kbd> This would be the textbook
solution.

```rust
// Bad: caller needs to write task.index() instead of task.index

pub struct Task {
    index: usize,
    // other private fields
}

impl Task {
    pub fn index(&self) -> usize {
        self.index
    }
}
```

For the ways that this API is commonly used as an argument to other function
calls, I felt that the additional method call parentheses from the getter would
be noisy and provide zero benefit. Rust users already understand how struct
fields work and would be happy to access this value as a field if I can let
them. From the role of this type in the crate's API it is very unlikely that
someone would want to mutate the field, but still we need to protect against it
for correctness.

<br>

### Background

The way `.` field access syntax works, if there is no field found with the right
name then the language will look at the type's `Deref` impl or a sequence of
`Deref` impls to determine the field being named. This behavior is important for
making smart pointers like `Box` convenient to use:

```rust
// Somewhere in the standard library:
//
// pub struct Box<T: ?Sized> {
//     ptr: *mut T,
// }

struct S {
    x: String,
}

fn f(s: Box<S>) {
    // Box<S> has no field called x so it isn't obvious why
    // this line would be legal, but Box<S> dereferences to
    // S which does have that field.
    println!("{}", s.x);
}
```

Importantly for encapsulation, the deref behavior takes place even if a field
with the right name exists on the original type but is private. Suppose that
`Box` were implemented by storing the heap pointer it owns in a private field
called `ptr`. In that case we would still want the following code to refer to
the user's `ptr` field, rather than erroring because `ptr` exists on `Box` and
is private:

```rust
struct S {
    ptr: *const u8,
}

fn f(s: Box<S>) {
    println!("{:p}", s.ptr);
}
```

The final detail relevant to our original use case is that fields accessed
through a `Deref` impl cannot be mutated unless the outer type also implements
`DerefMut`. The `Deref` method signature looks like `fn deref(&self) ->
&Self::Target` while the `DerefMut` signature looks like `fn deref_mut(&mut
self) -> &mut Self::Target`.

<br>

### First attempt

We can implement read-only fields by moving the state behind a `Deref` impl to a
type with the appropriate fields public. Without a `DerefMut` impl, this makes
all accessible fields read-only outside of the current module.

```rust
pub struct Task {
    inner: ReadOnlyTask,
}

pub struct ReadOnlyTask {
    pub index: usize,
    // other private fields
}

impl Deref for Task {
    type Target = ReadOnlyTask;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
```

This is pretty good from the point of view of downstream code. As intended, code
from outside the module can access `task.index` through deref but cannot mutate
`task.index`.

The big problem with this approach is that it distresses the borrow checker.
From inside the module, if code takes a reference to one of the private fields
through deref, say `&task.other`, deref gets a reference to the whole `&Task`
which precludes then mutating some different fields while retaining the
reference.

```console
error[E0506]: cannot assign to `task.inner.another` because it is borrowed
 --> src/main.rs:8:5
  |
7 |     let other = &task.other;
  |                  ---- borrow of `task.inner.another` occurs here
8 |     task.inner.another = 1;
  |     ^^^^^^^^^^^^^^^^^^^^^^ assignment to borrowed `task.inner.another` occurs here
```

To work around this, practically all code within the module would need to be
written in terms of `task.inner.*` explicitly rather than relying on derefs,
which is unpleasant.

<br>

### Second attempt

We can keep the original struct but dereference to a struct with the same memory
layout and public fields, still not implementing `DerefMut`.

For this to be sound, we need to guarantee that both copies of the struct have
the same layout in memory. This is *not* guaranteed just by having the same
fields with the same types in both. One way to do it is by using `#[repr(C)]` to
tie both structs to C's struct layout rules, because those do guarantee the same
layout for structs with identical fields.

```rust
#[repr(C)]
pub struct Task {
    index: usize,
    // other private fields
}

#[repr(C)]
pub struct ReadOnlyTask {
    pub index: usize,
    // the same private fields
}

impl Deref for Task {
    type Target = ReadOnlyTask;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self as *const Self as *const Self::Target) }
    }
}
```

This works as intended. Code from inside this module can access and mutate the
private `task.index` directly, while code from outside the module can access
`task.index` through `Deref` and cannot mutate it even if the `Task` they hold
is mutable.

```console
error[E0594]: cannot assign to data in a `&` reference
 --> main.rs:8:5
  |
8 |     task.index += 1;
  |     ^^^^^^^^^^^^^^^ cannot assign
```

But this is not a complete solution because we really want the field to appear
as a public field in Rustdoc so that readers of the documentation immediately
understand how to use it. The documentation experience should be as though this
field were declared `pub`.

<br>

### Third attempt

We can use `#[cfg(rustdoc)]` to distinguish when documentation is being
rendered, though this cfg is currently available on nightly only. The tracking
issue is [rust-lang/rust#43781].

[rust-lang/rust#43781]: https://github.com/rust-lang/rust/issues/43781

I ended up using a Cargo config to have rustdoc pass some different cfg.

```toml
[build]
rustdocflags = ["--cfg", "oqueue_docs"]
```

```rust
#[repr(C)]
pub struct Task {
    #[cfg(oqueue_docs)]
    pub index: usize,

    #[cfg(not(oqueue_docs))]
    index: usize,

    // other private fields
}

#[doc(hidden)]
#[repr(C)]
pub struct ReadOnlyTask {
    pub index: usize,
    // the same private fields
}

#[doc(hidden)]
impl Deref for Task {
    type Target = ReadOnlyTask;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self as *const Self as *const Self::Target) }
    }
}
```

This renders as intended in rustdoc as:

```console
pub struct Task {
    pub index: usize,
    // some fields omitted
}
```

so readers immediately know how to access the field. From the role of this type
in the crate's API it is unlikely that anyone would want to mutate the field,
but just in case, the field's documentation points out that it is read-only.

<br>

### Implementation

Once the right strategy for generated code has been worked out, [productizing
the behavior as an attribute macro][readonly] is the easy part:

[readonly]: https://github.com/dtolnay/readonly

```rust
/// ...
#[readonly::make(doc = oqueue_docs)]
pub struct Task {
    /// ...
    ///
    /// This field is read-only; writing to its value will not compile.
    pub index: usize,

    // other private fields
}
```
