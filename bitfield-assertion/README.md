## Multiple of 8 const assertion

We need a macro that will fail to compile if some expression is not a multiple
of 8, without knowing the value of the expression until after name resolution
which happens after macro expansion.

This came up in the context of bitfields where sizes of fields are specified in
bits but the application would like to require that the total size is an exact
number of bytes.

```rust
trait Field {
    const BITS: usize;
}

enum B3 {}
impl Field for B3 {
    const BITS: usize = 3;
}

enum B5 {}
impl Field for B5 {
    const BITS: usize = 5;
}

fn main() {
    require_multiple_of_eight!(B3::BITS + B5::BITS);
}
```

As always, we would like the error message to be as precise and useful as
possible even though in this case the macro does not control the exact message
because this error can only be detected after name resolution.

<br>

### First attempt

The two main ways a macro can trigger compile-time errors after macro expansion
are in const evaluation and in type checking.

Let's look at const evaluation first by writing a `const` that can be
successfully computed if and only if the input expression is a multiple of 8.
There are many ways to do this but one way is to use `$e % 8` as an index into
an array where the only legal index would be 0.

```rust
macro_rules! require_multiple_of_eight {
    ($e:expr) => {
        const REQUIRE_MULTIPLE_OF_EIGHT: () = [()][$e % 8];
        _ = REQUIRE_MULTIPLE_OF_EIGHT;
    };
}
```

This seems like it should get the job done but it doesn't quite. There are some
weird optimizations around const evaluation. In particular a `cargo check` would
not need to evaluate this constant. It does a simple type check only which
determines that *if* the constant does evaluate successfully then its type would
be `()` which matches the declared type so everything is okay. On the other hand
`cargo build` does need to perform the evaluation. We end up in a situation
where `cargo check` can succeed at the same time as `cargo build` fails, which
is not good.

Separately, this approach does not give us any opportunity to control the
message part of the error. If the same macro needed to evaluate multiple
assertions, the caller couldn't tell which one was failing.

The message looks like:

```console
error[E0080]: erroneous constant used
 --> src/main.rs:8:10
  |
8 | #[derive(Bitfield)]
  |          ^^^^^^^^ referenced constant has errors
```

<br>

### Second attempt

Let's use `$e` to produce something that only type checks if the given
expression is a multiple of 8.

Currently the only place that expressions can appear in the type grammar is in
the length of a fixed sized array, so we will rely on that.

```rust
macro_rules! require_multiple_of_eight {
    ($e:expr) => {
        _ = <[(); $e % 8] as $crate::MultipleOfEight>::check();
    };
}

trait MultipleOfEight {
    fn check() {}
}

impl MultipleOfEight for [(); 0] {}
```

This is pretty good! The array type `[(); $e % 8]` only implements the required
trait if `$e % 8` is zero. The trait solver's error message mentions
"MultipleOfEight" which adequately indicates to the user what went wrong.

```console
error[E0277]: the trait bound `[(); 6]: MultipleOfEight` is not satisfied
 --> src/main.rs:8:10
  |
8 | #[derive(Bitfield)]
  |          ^^^^^^^^ the trait `MultipleOfEight` is not implemented for `[(); 6]`
  |
  = help: the following implementations were found:
            <[(); 0] as MultipleOfEight>
  = note: required by `MultipleOfEight::check`
```

There are some things to improve upon though. The error message includes this
distracting array type `[(); 6]` that is not obviously related to what the
caller might have written. Also the note mentioning the method
`MultipleOfEight::check` is just noise as far as the caller would be concerned.

<br>

### Solution

Let's solve this without a method call and without the array type being the
thing with a missing trait impl.

```rust
macro_rules! require_multiple_of_eight {
    ($e:expr) => {
        let _: $crate::MultipleOfEight<[(); $e % 8]>;
    };
}

type MultipleOfEight<T> = <<T as Array>::Marker as TotalSizeIsMultipleOfEightBits>::Check;

enum ZeroMod8 {}
enum OneMod8 {}
enum TwoMod8 {}
enum ThreeMod8 {}
enum FourMod8 {}
enum FiveMod8 {}
enum SixMod8 {}
enum SevenMod8 {}

trait Array {
    type Marker;
}

impl Array for [(); 0] {
    type Marker = ZeroMod8;
}

impl Array for [(); 1] {
    type Marker = OneMod8;
}

impl Array for [(); 2] {
    type Marker = TwoMod8;
}

impl Array for [(); 3] {
    type Marker = ThreeMod8;
}

impl Array for [(); 4] {
    type Marker = FourMod8;
}

impl Array for [(); 5] {
    type Marker = FiveMod8;
}

impl Array for [(); 6] {
    type Marker = SixMod8;
}

impl Array for [(); 7] {
    type Marker = SevenMod8;
}

trait TotalSizeIsMultipleOfEightBits {
    type Check;
}

impl TotalSizeIsMultipleOfEightBits for ZeroMod8 {
    type Check = ();
}
```

In this code the `<T as Array>::Marker` always resolves to one of `ZeroMod8`
through `SevenMod8`. But then only `ZeroMod8` implements
`TotalSizeIsMultipleOfEightBits`.

Here is the error message, pretty helpful and free of the distractions from the
second attempt.

```console
error[E0277]: the trait bound `SixMod8: TotalSizeIsMultipleOfEightBits` is not satisfied
 --> src/main.rs:8:10
  |
8 | #[derive(Bitfield)]
  |          ^^^^^^^^ the trait `TotalSizeIsMultipleOfEightBits` is not implemented for `SixMod8`
```

<br>

### Future

Someone should write an RFC for const\_assert. Something like:

```rust
const_assert!($e % 8 == 0, "total size is required to be a multiple of 8 bits");
```

Having this provided by the compiler would let us give better error messages
with less effort than the solution above.
