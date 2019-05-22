## Unit struct with type parameters

[`PhantomData<T>`] is a lang item which means it is currently implemented using
dedicated logic in the compiler, but it turns out all of its behavior can be
implemented from ordinary Rust code. This gives a good opportunity to explore
namespaces in Rust name resolution.

[`PhantomData<T>`]: https://doc.rust-lang.org/std/marker/struct.PhantomData.html

The defining characteristic of `PhantomData` is that it is a unit struct with a
type parameter, which is not otherwise allowed by Rust.

```rust
struct MyPhantomData<T: ?Sized>;

fn main() {
    let _: MyPhantomData<usize> = MyPhantomData;
}
```

```console
error[E0392]: parameter `T` is never used
 --> src/main.rs:1:22
  |
1 | struct MyPhantomData<T: ?Sized>;
  |                      ^ unused parameter
  |
  = help: consider removing `T` or using a marker such as `std::marker::PhantomData`
```

This is a hard error, not a warning that can be suppressed like some other lints
about unused code. Rust needs to insist on all type parameters appearing somehow
in the data structure because it is critical for determining [variance].

[variance]: https://doc.rust-lang.org/nomicon/subtyping.html

We will develop an attribute macro to make this work by assuming covariance for
the type parameter the same as `PhantomData`. As always, the hard part is
figuring out what code to generate, not writing the macro.

```rust
#[phantom]
struct MyPhantomData<T: ?Sized>;

fn main() {
    let _: MyPhantomData<usize> = MyPhantomData;
}
```

Solving this functionality opens some interesting design possibilities for
libraries that want something that is usable like `PhantomData` but is a locally
defined type, meaning the library can control the impl of traits like
`IntoIterator` on it. The iteration API of [`inventory`] is an example of such a
type in a public crate.

[`inventory`]: https://github.com/dtolnay/inventory

<br>

### Background

Names of things in Rust exist in one of three namespaces:

- The type namespace: structs, enums, unions, traits, modules, enum variants.

- The value namespace: functions, local variables, statics, consts, tuple struct
  constructors, unit struct instances, tuple variant constructors, unit
  variants instances.

- The macro namespace: macro\_rules macros, function-like procedural macros,
  attribute macros, derive macros.

The following is not a precise rule, but the intuition is that something exists
in the type namespace if you can write:

```rust
let _: TYPE;
```

while something exists in the value namespace if you can write:

```rust
let _ = VALUE;
```

These two syntactic positions are always unambiguous in the Rust grammar, so
permitting the same name to refer to different things in each namespace does not
introduce ambiguity.

It is possible to have the same name refer to different things in all three
namespaces at once:

```rust
// X in the macro namespace
macro_rules! X {
    () => {};
}

// X in the type namespace
struct X {}

// X in the value namespace
const X: () = ();

fn main() {
    // unambiguously the macro X
    X!();

    // unambiguously the type X
    let _: X;

    // unambiguously the value X
    let _ = X;
}
```

Some definitions place a name into more than one namespace. For example unit
structs (`struct S;`) and tuple structs (`struct S(A, B);`) are both types and
values. The value corresponding to a unit struct is like a constant whose value
is that unit struct, and the value corresponding to a tuple struct is like a
function that takes the tuple elements and returns the tuple struct.

Braced structs (`struct S { a: A }`) are types only.

<br>

### Strategy

`PhantomData`, being a unit struct, consists of a type component and a value
component. When you write `use std::marker::PhantomData` you are importing both.

```rust
use std::marker::PhantomData;

fn main() {
    let _: PhantomData<usize> = PhantomData::<usize>;
}
```

In implementing our own `PhantomData` we will tackle the two namespaces one
after the other.

In the value namespace we will need something that makes the following valid:

```rust
fn main() {
    let _ = MyPhantomData::<usize>;
}
```

And in the type namespace we will need something for this:

```rust
fn main() {
    let _: MyPhantomData<usize>;
}
```

Independently these would be easy, but the hard part will be making it so that
`MyPhantomData::<usize>` as a value has a type that matches
`MyPhantomData<usize>`.

```rust
fn main() {
    let _: MyPhantomData<usize> = MyPhantomData::<usize>;
}
```

<br>

### Value namespace

In the value namespace basically our only tool relevant to this project is unit
variants. The other obvious candidates in the value namespace (statics and
consts) cannot carry a type parameter.

You may be familiar with type parameters on unit variants already, maybe without
thinking about it, from dealing with `Option`:

```rust
fn main() {
    let mut x = None::<usize>;

    // equivalent to:
    let mut x: Option<usize> = None;
}
```

Here is how we would make a unit variant with a type parameter that can be
imported and used in value position:

```rust
mod phantom {
    pub use self::ImplementationDetail::MyPhantomData;

    pub enum ImplementationDetail<T: ?Sized> {
        MyPhantomData,

        #[allow(dead_code)]
        #[doc(hidden)]
        Marker(*const T),
    }
}

use phantom::MyPhantomData;

fn main() {
    let _ = MyPhantomData::<usize>;
}
```

The marker variant is responsible for using the type parameter `T` in some way
that gives it the right variance. There are many correct alternatives but I made
it hold `*const T` as one example of a type that is covariant in `T` and works
with dynamically sized `T: ?Sized`. We will come back to autotrait impls later.

<br>

### Type namespace

Clearly in the previous section the type of the enum variant
`MyPhantomData::<usize>` is the enum type `ImplementationDetail<usize>`. We just
need to call it something else, namely `MyPhantomData<usize>`.

Changing the name doesn't immediately work.

```rust
mod phantom {
    pub use self::MyPhantomData::MyPhantomData;

    pub enum MyPhantomData<T: ?Sized> {
        MyPhantomData,

        #[allow(dead_code)]
        #[doc(hidden)]
        Marker(*const T),
    }
}
```

```console
error[E0255]: the name `MyPhantomData` is defined multiple times
 --> src/main.rs:4:5
  |
2 |     pub use self::MyPhantomData::MyPhantomData;
  |             ---------------------------------- previous import of the type `MyPhantomData` here
3 | 
4 |     pub enum MyPhantomData<T: ?Sized> {
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `MyPhantomData` redefined here
  |
  = note: `MyPhantomData` must be defined only once in the type namespace of this module
help: you can use `as` to change the binding name of the import
  |
2 |     pub use self::MyPhantomData::MyPhantomData as OtherMyPhantomData;
  |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

The behavior seen here is that all enum variants of any style (struct variant,
tuple variant, unit variant) occupy both the value namespace and the type
namespace. Our code had defined `enum MyPhantomData` as a type, but then
imported `self::MyPhantomData::MyPhantomData` which is both a value and type,
resulting in a conflict in the type namespace.

Naively we might expect that unit variants and tuple variants occupy only the
value namespace while struct variants occupy only the type namespace. Unit
variants necessarily need something in the value namespace through which you
refer to their value, and tuple variants necessarily need something in the value
namespace that behaves like a function through which you construct them. And
struct variants need something to make curly brace initialization work, which
seems like it should be the type namespace because plain structs with named
fields exist in the type namespace only. But apparently this is not how things
work -- maybe to leave things open for language evolution in which enum variants
become usable as refinement types.

In any case, the way to work around conflicts is via wildcard imports. These are
allowed to overlap with non-wildcard imports or explicit definitions, in which
case the non-wildcard takes precedence. The precedence applies independently
within each namespace.

```rust
mod phantom {
    // Imports the enum variant in both type and value namespace,
    // but in the type namespace it gets shadowed by the definition
    // `enum MyPhantomData` below.
    pub use self::MyPhantomData::*;

    pub enum MyPhantomData<T: ?Sized> {
        MyPhantomData,

        #[allow(dead_code)]
        #[doc(hidden)]
        Marker(*const T),
    }
}

use phantom::MyPhantomData;

fn main() {
    let _: MyPhantomData<usize> = MyPhantomData::<usize>;
}
```

Pretty neat! There are some quirks to sort out still, but this is on the right
track.

<br>

### Memory representation

We want `std::mem::size_of::<MyPhantomData<T>>() == 0`.

In the definition above, it would currently be a whopping 16 or 24 bytes
depending on whether `T` is dynamically sized. The marker variant takes up space
for a pointer or fat pointer, and there is an enum discriminant as well which
needs 1 bit, and we get a further 63 bits of padding for alignment reasons.

Two things need to change: we need the marker variant not to contain storage,
and we need the discriminant not to exist.

We can eliminate the discriminant by making the marker variant's data zero sized
and statically impossible. The compiler is smart enough to elide the
discriminant when this happens.

For various complicated but reasonably good reasons, just making the data
impossible without making it zero sized (such as `Marker(Void, *const T)`) is
not sufficient.

```rust
mod phantom {
    pub use self::MyPhantomData::*;

    pub enum MyPhantomData<T: ?Sized> {
        MyPhantomData,

        #[allow(dead_code)]
        #[doc(hidden)]
        Marker(Void, [*const T; 0]),
    }

    pub enum Void {}
}

use phantom::MyPhantomData;

fn main() {
    assert_eq!(std::mem::size_of::<MyPhantomData<usize>>(), 0);
}
```

<br>

### Autotraits

The standard library's `PhantomData<T>` has `impl<T: ?Sized + Send> Send` and
`impl<T: ?Sized + Sync> Sync`. Our type so far has neither of these because
`*const T` does not.

A simple fix would be `Marker(Void, [Box<T>; 0])` but then we depend on a memory
allocator for no reason. This fix works because `Box<T>` has the same `Send` and
`Sync` impls as `T`.

Without `Box`, the same impls can be written unsafely.

```rust
mod phantom {
    pub use self::MyPhantomData::*;

    pub enum MyPhantomData<T: ?Sized> {
        MyPhantomData,

        #[allow(dead_code)]
        #[doc(hidden)]
        Marker(Void, [*const T; 0]),
    }

    pub enum Void {}

    unsafe impl<T: ?Sized + Send> Send for MyPhantomData<T> {}
    unsafe impl<T: ?Sized + Sync> Sync for MyPhantomData<T> {}
}
```

<br>

### Documentation

Rustdoc would render our type as:

```console
pub enum MyPhantomData<T: ?Sized> {
    MyPhantomData,
    // some variants omitted
}
```

which is technically accurate, but misleading relative to how we want users to
conceptualize this construct.

There isn't a great solution to this, but you may or may not find the following
more appealing:

```rust
mod phantom {
    pub use self::MyPhantomData::*;

    pub enum MyPhantomData<T: ?Sized> {
        MyPhantomData,

        #[allow(dead_code)]
        #[doc(hidden)]
        Marker(Void, [*const T; 0]),
    }

    pub enum Void {}

    unsafe impl<T: ?Sized + Send> Send for MyPhantomData<T> {}
    unsafe impl<T: ?Sized + Sync> Sync for MyPhantomData<T> {}
}

/// ... documentation illustrating how to use.
#[allow(type_alias_bounds)]
pub type MyPhantomData<T: ?Sized> = phantom::MyPhantomData<T>;

#[doc(hidden)]
pub use self::phantom::*;
```

Rustdoc renders:

```console
type MyPhantomData<T: ?Sized> = MyPhantomData<T>;
```

which hides the implementation detail and drives focus to your handwritten
documentation to show how the type is intended to be used.

The `#[allow(type_alias_bounds)]` attribute suppresses a future compatibility
lint that triggers on type aliases with trait bounds on the left hand side. The
Rust compiler currently does not respect such bounds but this behavior is
considered a compiler bug and is subject to change, potentially breaking code
involving trait bounds in type aliases -- hence the lint. Our code above is in
the clear because the bounds in the type alias exactly match the bounds implied
by well-formedness of the right hand side, so the meaning is the same whether or
not the compiler looks at the type alias bounds. We want the bounds there
because they do appear correctly in Rustdoc.

<br>

### Implementation

Once the generated code is figured out, packaging this into [an attribute
macro][ghost] is the easy part.

[ghost]: https://github.com/dtolnay/ghost

```rust
/// ... documentation illustrating how to use.
#[phantom]
struct MyPhantomData<T: ?Sized>;
```

In fact we might as well make it work for any number of type parameters and
lifetimes, as well as trait bounds and where-clauses.

```rust
#[phantom]
struct Crazy<'a, V: 'a, T> where &'a V: IntoIterator<Item = T>;
```
