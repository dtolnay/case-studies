## Autoref-based stable specialization

"Specialization" refers to permitting overlapping impls in Rust's trait system
so long as for every possible type, one of the applicable impls is "more
specific" than the others for some intuitive but precisely defined notion of
specific. Discussions about a specialization language feature have been ongoing
for 4.5 years ([RFC 1210], [rust-lang/rust#31844]). Today the feature is
partially implemented in rustc but is not yet sound when mixed with lifetimes
([rust-lang/rust#40582]) and requires more language design work and compiler
work before it could be stabilized.

[RFC 1210]: https://github.com/rust-lang/rfcs/pull/1210
[rust-lang/rust#31844]: https://github.com/rust-lang/rust/issues/31844
[rust-lang/rust#40582]: https://github.com/rust-lang/rust/issues/40582

This page covers a stable, safe, generalizable technique for solving some of the
use cases that would otherwise be blocked on specialization.

The technique was originally developed for use by macros in the [Anyhow] crate.

[Anyhow]: https://github.com/dtolnay/anyhow

<br>

### Context

I'll explain the technique as applied to two use cases, one simpler to start
with and then a more elaborate realistic one.

The first use case is going to be a truly canonical application of
specialization &mdash; a blanket impl with a separate fast path for some
concrete type(s). The equivalent nightly-only specialized blanket impl would be
like this:

```rust
#![feature(specialization)]

use std::fmt::{Display, Write};

pub trait MyToString {
    fn my_to_string(&self) -> String;
}

// General impl that applies to any T with a Display impl.
impl<T: Display> MyToString for T {
    default fn my_to_string(&self) -> String {
        let mut buf = String::new();
        buf.write_fmt(format_args!("{}", self)).unwrap();
        buf.shrink_to_fit();
        buf
    }
}

// Specialized impl to bypass the relatively expensive std::fmt machinery.
impl MyToString for String {
    fn my_to_string(&self) -> String {
        self.clone()
    }
}
```

Then the second use case will be closer to the real-life usage of this technique
in Anyhow. We have an error type, and we want it to be constructible from any
underlying type that has a `Display` impl. But if the underlying type *also* has
a `std::error::Error` impl, we'd like to know about that by invoking a different
constructor which will propagate the original error's source() and backtrace()
information correctly.

Ultimately we want both of the following to compile:

```rust
fn demo1() -> Result<(), anyhow::Error> {
    // Turn a &str into an error.
    // &str implements Display but not std::error::Error.
    return Err(anyhow!("oh no!"));
}

fn demo2() -> Result<(), anyhow::Error> {
    // Turn an existing std::error::Error value into our error without
    // losing its source() and backtrace() if there is one.
    let io_error = fs::read("/tmp/nonexist").unwrap_err();
    return Err(anyhow!(io_error));
}
```

Recall that `std::error::Error` has `Display` as a supertrait so the impl for
`std::error::Error` is strictly more specific than the general impl that covers
all `Display` types.

```rust
#![feature(specialization)]

use std::error::Error as StdError;
use std::fmt::Display;

pub struct Error(/* ... */);

impl Error {
    pub(crate) fn from_fmt<T: Display>(error: T) -> Self {...}
    pub(crate) fn from_std_error<T: StdError>(error: T) -> Self {...}
}

pub(crate) trait AnyhowNew {
    fn new(self) -> Error;
}

impl<T: Display> AnyhowNew for T {
    default fn new(self) -> Error {
        // no std error impl
        Error::from_fmt(self)
    }
}

impl<T: StdError> AnyhowNew for T {
    fn new(self) -> Error {
        // able to use std error's source() and backtrace()
        Error::from_std_error(self)
    }
}
```

<br>

### Background: autoref

To do specialization using only 100% stable and 100% safe code, we'll need some
other mechanism to accomplish compile-time fallback through a prioritized
sequence of behaviors. That is, we need some way to define a general impl and a
tree of more specific impls where any invocation will resolve to the most
specific applicable impl at compile time.

Outside of `feature(specialization)`, Rust has at least one other language
feature capable of doing this, which is method resolution autoref.

As an introduction to autoref let's consider this program:

```rust
struct Value(i32);

impl Value {
    fn print(&self) {
        println!("it worked! {}", self.0);
    }
}

fn main() {
    let v = Value(0);
    v.print();
}
```

We make a variable `v` of type `Value` and call a method on it. If you've
written any Rust code it will be obvious to you *that* this code works, but I'd
like to dig into *why* it works. In particular, we have a value of type `Value`
but the method `print` takes an argument of type `&Value`. Where is the code
that turns `Value` into `&Value`?

This is autoref &mdash; the compiler is inserting the required reference for you
as part of resolving the method call. In effect, the code that executes is
equivalent to if we had written `(&v).print()` or more explicitly
`Value::print(&v)`, but it is "auto" because we never had to write `&` in the
call.

Note: autoref is not the same as deref, which is a different thing that method
resolution does. In a way they are opposites; autoref is about *adding* a layer
of reference to resolve a call; deref is about *removing* a layer of reference.
Both are ubiquitous but invisible.

<br>

### Background: method resolution

How does autoref get us stable specialization? To answer that, let's look at
what happens if the same method name could be dispatched either with or without
autoref.

```rust
struct Value;

trait Print {
    fn print(self);
}

impl Print for Value {
    fn print(self) {
        println!("called on Value");
    }
}

impl Print for &Value {
    fn print(self) {
        println!("called on &Value");
    }
}

fn main() {
    let v = Value;
    v.print();
}
```

Here `print` could refer to either `<Value as Print>::print` which takes an
argument of type `Value`, or to `<&Value as Print>::print` which takes an
argument of type `&Value`. If you run this program you'll see it prints "called
on Value". But if the first impl were removed, it would then print "called on
&amp;Value". In some sense the first impl is more specific from the point of
view of the call we wrote; exactly what we'll need!

To define the compiler's behavior more precisely, the rule is that if a method
can be dispatched without autoref then it will be. Only if a method cannot be
dispatched without autoref, the compiler will insert an autoref and attempt to
resolve it again.

This and some creativity should be all we need to solve the use cases that we
saw up top.

<br>

### Simple application

Recall that we have a String conversion that we wanted to implement in one way
for any `T: Display` and in a more performant specialized way for specifically
`String`.

Here is the full implementation:

```rust
use std::fmt::{Display, Write};

pub trait DisplayToString {
    fn my_to_string(&self) -> String;
}

// General impl that applies to any T with a Display impl.
//
// Note that the Self type of this impl is &T and so the method argument
// is actually &&T! That makes this impl lower priority during method
// resolution if the impl that accepts &String would also apply.
impl<T: Display> DisplayToString for &T {
    fn my_to_string(&self) -> String {
        println!("called blanket impl");

        let mut buf = String::new();
        buf.write_fmt(format_args!("{}", self)).unwrap();
        buf.shrink_to_fit();
        buf
    }
}

pub trait StringToString {
    fn my_to_string(&self) -> String;
}

// Specialized impl to bypass the relatively expensive std::fmt machinery.
//
// The method argument is typed &String.
impl StringToString for String {
    fn my_to_string(&self) -> String {
        println!("called specialized impl");

        self.clone()
    }
}

macro_rules! convert_to_strings {
    ($($e:expr),*) => {
        [$(
            (&$e).my_to_string()
        ),*]
    };
}

fn main() {
    let owned_string = "hacks".to_owned();
    let strings = convert_to_strings![1, "&str", owned_string];
    println!("{:?}", strings);
}
```

If we run this program the output shows that our specialization works!

```console
called blanket impl
called blanket impl
called specialized impl
["1", "&str", "hacks"]
```

<br>

### Realistic application

Recall that we have an Error type that we'd like to construct from any `T` that
implements `Display`, but using a different constructor if `T` also implements
`std::error::Error`.

The reason this is more complicated than the previous use case is that my Error
constructors want to receive the argument *by value*! That's bad news if we are
relying on autoref because autoref is all about inserting a layer of reference.

Instead we'll use a tagged dispatch strategy with a pair of method calls, the
first using autoref-based specialization with a reference argument to select a
tag, and the second based on that tag which takes ownership of the original
argument.

```rust
use std::error::Error as StdError;
use std::fmt::Display;

pub struct Error(/* ... */);

// Our two constructors. The first is more general.
impl Error {
    pub(crate) fn from_fmt<T: Display>(error: T) -> Self {
        println!("called Error::from_fmt");
        Error {}
    }
    pub(crate) fn from_std_error<T: StdError>(error: T) -> Self {
        _ = error.source(); // it works!
        println!("called Error::from_std_error");
        Error {}
    }
}

macro_rules! anyhow {
    ($err:expr) => ({
        #[allow(unused_imports)]
        use $crate::{DisplayKind, StdErrorKind};
        match $err {
            error => (&error).anyhow_kind().new(error),
        }
    });
}

// If the arg implements Display but not StdError, anyhow_kind() will
// return this tag.
struct DisplayTag;

trait DisplayKind {
    #[inline]
    fn anyhow_kind(&self) -> DisplayTag {
        DisplayTag
    }
}

// Requires one extra autoref to call! Lower priority than StdErrorKind.
impl<T: Display> DisplayKind for &T {}

impl DisplayTag {
    #[inline]
    fn new<M: Display>(self, message: M) -> Error {
        Error::from_fmt(message)
    }
}

// If the arg implements StdError (and thus also Display), anyhow_kind()
// will return this tag.
struct StdErrorTag;

trait StdErrorKind {
    #[inline]
    fn anyhow_kind(&self) -> StdErrorTag {
        StdErrorTag
    }
}

// Does not require any autoref if called as (&error).anyhow_kind().
impl<T: StdError> StdErrorKind for T {}

impl StdErrorTag {
    #[inline]
    fn new<E: StdError>(self, error: E) -> Error {
        Error::from_std_error(error)
    }
}

fn main() {
    // Turn a &str into an error.
    // &str implements Display but not std::error::Error.
    let _err = anyhow!("oh no!");

    // Turn an existing std::error::Error value into our error without
    // losing its source() and backtrace() if there is one.
    let io_error = std::fs::read("/tmp/nonexist").unwrap_err();
    let _err = anyhow!(io_error);
}
```

<br>

### Limitations

The way that this technique applies method resolution cannot be described by a
trait bound, so for practical purposes you should think of this technique as
working in macros only.

That is, we can't do:

```rust
pub fn demo<T: ???>(value: T) -> String {
    (&value).my_to_string()
}
```

and get the specialized behavior. If we put `T: Display` in the trait bound,
method resolution will use the impl for `T: Display` even if `T` happened to be
instantiated as `String`.

Depending on your use case, this is honestly fine! If you are a macro already
then you're all set. If you can be made a macro, that's good too (like I did for
`anyhow!` (though it was good for that to be a macro anyway so that it can
accept format args the way println does)). If you can't possibly be a macro then
this won't help you.

I am excited to hear other people's experience applying this technique and I
expect it to generalize quite well.
