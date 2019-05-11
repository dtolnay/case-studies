## Consecutive integer match patterns

This came up in a macro that wanted to take a comma-separated sequence of
expressions like `themacro!('A', 'B', f())` and emit a `match` expression indexed by
position in the sequence:

```rust
match VALUE {
    0 => 'A',
    1 => 'B',
    2 => f(),
    _ => unimplemented!(),
}
```

As a macro\_rules macro, a core limitation was that we can't make identifiers
dynamically, so the generated code would be limited to using some fixed number
of identifiers regardless of how many expressions are in the macro input.

In the actual use case, this `match` was just one part of a more complicated
macro; we wouldn't necessarily want a macro for doing literally what is
described here by itself.

<br>

### Rejected solutions

<kbd>**Procedural macro.**</kbd> The whole thing could have been made a
procedural macro instead. A procedural macro would be able to emit exactly a
match expression as shown above. However the stable Rust compiler does not yet
support calling procedural macros in expression position, so the procedural
macro would have needed to be restricted to nightly only. Also it would mean
pulling in some extra dependencies for parsing.

<kbd>**Change input syntax.**</kbd> The input syntax for the macro could have
been changed to require the caller to pass their own counter in the input:
something like `themacro!((0, 'A'), (1, 'B'), (2, f()))`. This makes things easy
for the macro implementation but at the expense of the caller, which was the
wrong tradeoff. Here is what that would look like implemented:

```rust
// Force caller to provide their own counter.
macro_rules! themacro {
    ($(($i:pat, $e:expr)),*) => {
        match VALUE {
            $($i => $e,)*
            _ => unimplemented!(),
        }
    };
}
```

<br>

### Good solutions

<kbd>**If-else chain.**</kbd> We can make the macro expand to a chain of if-else
comparisons structured like this, with a counter in a local variable:

```rust
{
    let _value = VALUE;
    let mut _i = 0;
    if {
        let eq = _value == _i;
        _i += 1;
        eq
    } {
        $e
    } else if {
        let eq = _value == _i;
        _i += 1;
        eq
    } {
        $e
    } else if {
        let eq = _value == _i;
        _i += 1;
        eq
    } {
        $e
    } else {
        unimplemented!()
    }
}
```

The conditions of the `if` are equivalent to `_value == _i++` except that unary
increment does not exist in Rust.

The leading underscore in the local variables `_value` and `_i` is meaningful in
that it suppresses some of the compiler's lints on unused variables, unused
assignment, and unused mut. If the caller's sequence of expressions is empty,
then `_value` and `_i` are never read and `_i` is never mutated. If the caller's
sequence of expressions is nonempty, the value written to `_i` by the last `_i
+= 1` is never read. We could alternatively use `#[allow(unused_variables,
unused_mut, unused_assignments)]` but placing these attributes in a way that
they apply correctly to the macro-generated local variables but not to the
caller's $e expressions makes things more complicated.

Notice that the way the if-else chain is structured there is a clear chunk of
repeating tokens -- each `if` through the following `else`. That repeating
structure makes it very easy for this to be generated from a macro\_rules macro
in one step of expansion.

```rust
macro_rules! themacro {
    ($($e:expr),*) => {{
        let value = VALUE;
        let mut i = 0;
        $(
            if {
                let eq = value == i;
                i += 1;
                eq
            } {
                $e
            } else
        )* {
            unimplemented!()
        }
    }};
}
```

<br>

<kbd>**Const counter.**</kbd> In some situations we may really want to stick
with a `match` expression rather than an if-else chain, for example if the value
being matched is just part of a larger data structure and we need to bind other
parts of the data structure by-move in the same match.

We can't expand to a `match` in which the patterns are integer literals `0`,
`1`, `2` etc as shown in the introduction, at least not while supporting an
arbitrary number of input expressions, because macro\_rules can only copy and
paste tokens around, never come up with new tokens. If the caller passes 9999
input expressions, there wouldn't be any way for a macro\_rules macro to conjure
up a `9998` integer literal token to place in the output.

We also can't expand to arithmetic patterns because this is not legal Rust
syntax.

```rust
match VALUE {
    0 => $e,
    0 + 1 => $e,
    0 + 1 + 1 => $e,
    ...
}
```

Instead we will make generated code that looks like this:

```rust
{
    mod m {
        pub const X: usize = 0;
        pub mod m {
            pub const X: usize = super::X + 1;
            pub mod m {
                pub const X: usize = super::X + 1;
            }
        }
    }
    match VALUE {
        m::X => $e,
        m::m::X => $e,
        m::m::m::X => $e,
        _ => unimplemented!(),
    }
}
```

The nested modules here provide a way to avoid needing unique names for each
const, which macro\_rules wouldn't be able to create.

Figuring out the right generated code is the hard part. The macro implementation
ends up being an unremarkable tt-muncher macro that produces one layer of the
nesting at a time.

```rust
macro_rules! themacro {
    ($($v:expr),*) => {
        $crate::themacro_helper! {
            path: (m::X)
            def: ()
            arms: ()
            $($v),*
        }
    };
}

macro_rules! themacro_helper {
    (
        path: ($($path:tt)*)
        def: ($($def:tt)*)
        arms: ($(($i:pat, $v:expr))*)
    ) => {{
        #[allow(dead_code)]
        mod m {
            pub const X: usize = 0;
            $($def)*
        }
        match VALUE {
            $(
                $i => $v,
            )*
            _ => unimplemented!(),
        }
    }};
    (
        path: ($($path:tt)*)
        def: ($($def:tt)*)
        arms: ($(($i:pat, $v:expr))*)
        $next:expr $(, $rest:expr)*
    ) => {
        $crate::themacro_helper! {
            path: (m::$($path)*)
            def: (pub mod m { pub const X: usize = super::X + 1; $($def)* })
            arms: ($(($i, $v))* ($($path)*, $next))
            $($rest),*
        }
    };
}
```
