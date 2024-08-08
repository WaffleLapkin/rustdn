//! Module with std-like things, in other words "utilities".

pub trait AnyExt: Sized {
    fn apply<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }

    #[expect(unused)]
    fn also(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }
}

impl<T> AnyExt for T {}
