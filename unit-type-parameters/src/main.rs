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

fn main() {
    let _: MyPhantomData<usize> = MyPhantomData::<usize>;
}
