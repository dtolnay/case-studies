#[macro_export]
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

#[macro_export]
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

fn main() {
    const VALUE: usize = 2;
    dbg!(VALUE);
    dbg!(themacro!('A', 'B', 'C'));
}
